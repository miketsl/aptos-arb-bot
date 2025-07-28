use market_data_ingestor::recording_monitor::RecordingStats;
use market_data_ingestor::monitoring::MetricsCollector;
use market_data_ingestor::steps::detector_push::{DetectorPushStep, QueueOperationMetrics};
use common::types::DetectorMessage;
use tokio::sync::mpsc;
use tokio::time::Duration;
use std::time::Instant;

#[test]
fn test_queue_depth_calculation() {
    let mut stats = RecordingStats::new();
    
    // Test initial state
    assert_eq!(stats.queue_depth_current, 0);
    assert_eq!(stats.queue_depth_max_observed, 0);
    assert_eq!(stats.queue_operations_total, 0);
    
    // Test first operation
    stats.record_queue_metrics(5, 2.0, false);
    assert_eq!(stats.queue_depth_current, 5);
    assert_eq!(stats.queue_depth_max_observed, 5);
    assert_eq!(stats.queue_operations_total, 1);
    
    // Test higher depth
    stats.record_queue_metrics(10, 3.0, false);
    assert_eq!(stats.queue_depth_current, 10);
    assert_eq!(stats.queue_depth_max_observed, 10);
    assert_eq!(stats.queue_operations_total, 2);
    
    // Test lower depth (max should remain)
    stats.record_queue_metrics(7, 1.5, false);
    assert_eq!(stats.queue_depth_current, 7);
    assert_eq!(stats.queue_depth_max_observed, 10); // Should not decrease
    assert_eq!(stats.queue_operations_total, 3);
}

#[test]
fn test_queue_saturation_detection() {
    let mut stats = RecordingStats::new();
    
    // Test normal operation (no saturation)
    stats.record_queue_metrics(100, 2.0, false); // 100 < 800 (80% of 1000)
    assert_eq!(stats.queue_saturation_events, 0);
    
    // Test saturation event  
    stats.record_queue_metrics(850, 5.0, false); // 850 > 800 (80% of 1000)
    assert_eq!(stats.queue_saturation_events, 1);
    
    // Test multiple saturation events
    stats.record_queue_metrics(900, 7.0, false);
    stats.record_queue_metrics(950, 10.0, false);
    assert_eq!(stats.queue_saturation_events, 3);
}

#[test]
fn test_backpressure_duration_tracking() {
    let mut stats = RecordingStats::new();
    
    // Test non-blocking operations
    stats.record_queue_metrics(10, 1.0, false);
    stats.record_queue_metrics(15, 2.0, false);
    assert_eq!(stats.backpressure_duration_ms, 0.0);
    
    // Test blocking operation
    stats.record_queue_metrics(20, 50.0, true); // 50ms blocking operation
    assert!(stats.backpressure_duration_ms > 0.0);
    
    // Test rolling average of backpressure
    stats.record_queue_metrics(25, 100.0, true); // Another blocking operation
    
    // Should be rolling average: (50 + 100) / 4 total operations = 37.5
    let expected_avg = (50.0 + 100.0) / 4.0;
    assert!((stats.backpressure_duration_ms - expected_avg).abs() < 0.1);
}

#[tokio::test]
async fn test_queue_operation_metrics() {
    let metrics = QueueOperationMetrics::new(5.5, 10, true, 100);
    
    assert_eq!(metrics.operation_time_ms, 5.5);
    assert_eq!(metrics.queue_depth_before, 10);
    assert_eq!(metrics.was_blocked, true);
    assert_eq!(metrics.channel_capacity, 100);
}

#[tokio::test]
async fn test_detector_push_queue_monitoring() {
    let (sender, mut receiver) = mpsc::channel(10);
    let detector_push = DetectorPushStep::new(sender);
    
    // Test basic push with metrics
    let msg = DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: chrono::Utc::now(),
    };
    
    let metrics = detector_push.push_with_metrics(msg).await
        .expect("Failed to push message");
    
    // Verify metrics are reasonable
    assert!(metrics.operation_time_ms >= 0.0);
    assert!(metrics.operation_time_ms < 10.0); // Should be very fast
    assert!(metrics.queue_depth_before <= 10); // Within channel capacity
    assert_eq!(metrics.channel_capacity, 10);
    
    // Verify message was received
    let received = receiver.recv().await.expect("Message not received");
    match received {
        DetectorMessage::BlockStart { block_number, .. } => assert_eq!(block_number, 1),
        _ => panic!("Wrong message type received"),
    }
}

#[tokio::test]
async fn test_backpressure_detection() {
    // Create a small channel to force backpressure
    let (sender, _receiver) = mpsc::channel(2);
    let detector_push = DetectorPushStep::new(sender);
    
    // Fill the channel
    let msg1 = DetectorMessage::BlockStart { block_number: 1, timestamp: chrono::Utc::now() };
    let msg2 = DetectorMessage::BlockStart { block_number: 2, timestamp: chrono::Utc::now() };
    
    let metrics1 = detector_push.push_with_metrics(msg1).await
        .expect("Failed to push first message");
    let metrics2 = detector_push.push_with_metrics(msg2).await
        .expect("Failed to push second message");
    
    // Channel should be near capacity
    assert!(metrics1.queue_depth_before <= 2);
    assert!(metrics2.queue_depth_before <= 2);
    
    // Third message should cause backpressure (channel is full)
    let msg3 = DetectorMessage::BlockStart { block_number: 3, timestamp: chrono::Utc::now() };
    let start_time = Instant::now();
    
    // This should block or take longer due to full channel
    let result = tokio::time::timeout(Duration::from_millis(100), 
                                    detector_push.push_with_metrics(msg3)).await;
    
    // Either timeout or detect blocking
    match result {
        Ok(Ok(metrics)) => {
            // If it succeeded, it should have detected blocking
            assert!(metrics.was_blocked || metrics.operation_time_ms > 1.0);
        }
        Ok(Err(_)) => {
            // Send failed, which also indicates blocking
        }
        Err(_) => {
            // Timeout is also acceptable as it indicates blocking
        }
    }
}

#[tokio::test]
async fn test_queue_metrics_prometheus_integration() {
    let collector = MetricsCollector::new().expect("Failed to create metrics collector");
    
    // Update queue metrics
    collector.update_queue_metrics(25, 50, 3, 15.5);
    
    let metrics = collector.registry().gather();
    
    // Check queue depth current
    let queue_depth_metric = metrics.iter()
        .find(|m| m.get_name() == "mdi_queue_depth_current")
        .expect("Queue depth metric not found");
    assert_eq!(queue_depth_metric.get_metric()[0].get_gauge().get_value(), 25.0);
    
    // Check max queue depth
    let max_depth_metric = metrics.iter()
        .find(|m| m.get_name() == "mdi_queue_depth_max_observed")
        .expect("Max queue depth metric not found");
    assert_eq!(max_depth_metric.get_metric()[0].get_gauge().get_value(), 50.0);
    
    // Check saturation events
    let saturation_metric = metrics.iter()
        .find(|m| m.get_name() == "mdi_queue_saturation_events_total")
        .expect("Saturation events metric not found");
    assert_eq!(saturation_metric.get_metric()[0].get_counter().get_value(), 3.0);
}

#[tokio::test]
async fn test_concurrent_queue_operations() {
    use std::sync::Arc;
    use tokio::task;
    
    let (sender, mut receiver) = mpsc::channel(100);
    let detector_push = Arc::new(DetectorPushStep::new(sender));
    let mut handles = vec![];
    
    // Spawn concurrent push operations
    for i in 0..20 {
        let detector_push_clone = Arc::clone(&detector_push);
        let handle = task::spawn(async move {
            let msg = DetectorMessage::BlockStart {
                block_number: i,
                timestamp: chrono::Utc::now(),
            };
            
            detector_push_clone.push_with_metrics(msg).await
        });
        handles.push(handle);
    }
    
    // Collect all results
    let mut all_metrics = vec![];
    for handle in handles {
        let metrics = handle.await.expect("Task failed").expect("Push failed");
        all_metrics.push(metrics);
    }
    
    // Verify all operations completed
    assert_eq!(all_metrics.len(), 20);
    
    // Verify metrics are reasonable
    for metrics in &all_metrics {
        assert!(metrics.operation_time_ms >= 0.0);
        assert!(metrics.queue_depth_before <= 100);
        assert_eq!(metrics.channel_capacity, 100);
    }
    
    // Verify all messages were received
    let mut received_count = 0;
    while let Ok(_) = receiver.try_recv() {
        received_count += 1;
    }
    assert_eq!(received_count, 20);
}

#[test]
fn test_queue_utilization_calculations() {
    let mut stats = RecordingStats::new();
    
    // Test utilization with various queue states
    stats.record_queue_metrics(25, 2.0, false);  // 25 depth
    stats.record_queue_metrics(50, 3.0, false);  // 50 max depth
    stats.record_queue_metrics(30, 2.5, false);  // 30 current
    
    assert_eq!(stats.queue_depth_current, 30);
    assert_eq!(stats.queue_depth_max_observed, 50);
    assert_eq!(stats.queue_operations_total, 3);
    
    // Calculate utilization: current/max = 30/50 = 60%
    let utilization = if stats.queue_depth_max_observed > 0 {
        (stats.queue_depth_current as f64 / stats.queue_depth_max_observed as f64) * 100.0
    } else {
        0.0
    };
    assert_eq!(utilization, 60.0);
}

#[tokio::test]
async fn test_queue_performance_impact() {
    let iterations = 1000;
    let mut stats = RecordingStats::new();
    
    let start_time = Instant::now();
    
    // Measure queue metrics recording performance
    for i in 0..iterations {
        stats.record_queue_metrics(
            i % 100,           // queue depth
            i as f64 * 0.01,   // operation time
            i % 10 == 0,       // blocked
        );
    }
    
    let duration = start_time.elapsed();
    let avg_time_per_operation = duration.as_micros() as f64 / iterations as f64;
    
    // Should be very fast (< 1μs per operation)
    assert!(avg_time_per_operation < 1.0, 
           "Queue metrics too slow: {:.2}μs per operation", avg_time_per_operation);
    
    println!("Queue metrics performance: {:.2}μs per operation", avg_time_per_operation);
}

#[tokio::test]
async fn test_queue_memory_efficiency() {
    let mut stats = RecordingStats::new();
    let initial_size = std::mem::size_of_val(&stats);
    
    // Add many queue operations
    for i in 0..10000 {
        stats.record_queue_metrics(i % 1000, i as f64 * 0.001, i % 100 == 0);
    }
    
    let final_size = std::mem::size_of_val(&stats);
    
    // Memory usage should not grow (fixed-size struct)
    assert_eq!(initial_size, final_size, "Memory usage should remain constant");
    
    // Verify metrics are still accurate
    assert_eq!(stats.queue_operations_total, 10000);
    assert_eq!(stats.queue_depth_max_observed, 999);
    assert!(stats.queue_saturation_events > 0); // Some operations should have triggered saturation
}

#[test]
fn test_queue_edge_cases() {
    let mut stats = RecordingStats::new();
    
    // Test zero depth
    stats.record_queue_metrics(0, 1.0, false);
    assert_eq!(stats.queue_depth_current, 0);
    assert_eq!(stats.queue_depth_max_observed, 0);
    
    // Test very high depth
    stats.record_queue_metrics(999999, 100.0, true);
    assert_eq!(stats.queue_depth_current, 999999);
    assert_eq!(stats.queue_depth_max_observed, 999999);
    assert!(stats.queue_saturation_events > 0); // Should trigger saturation
    
    // Test zero operation time
    stats.record_queue_metrics(10, 0.0, false);
    // Should not panic or cause issues
    
    // Test negative operation time (shouldn't happen but test robustness)
    stats.record_queue_metrics(5, -1.0, false);
    // Should handle gracefully
}

#[tokio::test]
async fn test_production_queue_metrics_integration() {
    let mut collector = MetricsCollector::new().expect("Failed to create metrics collector");
    let mut stats = RecordingStats::new();
    
    // Simulate realistic queue operations
    stats.record_queue_metrics(10, 2.5, false);
    stats.record_queue_metrics(25, 5.0, false);
    stats.record_queue_metrics(850, 15.0, true); // Saturation event
    stats.record_queue_metrics(15, 3.0, false);
    
    // Update metrics collector
    collector.update_metrics(&stats, "test", true);
    
    // Get production metrics
    let production_metrics = collector.get_production_metrics("test", true);
    
    // Verify queue monitoring metrics
    assert_eq!(production_metrics.queue_monitoring.queue_depth_current, 15);
    assert_eq!(production_metrics.queue_monitoring.queue_depth_max_observed, 850);
    assert_eq!(production_metrics.queue_monitoring.queue_saturation_events, 1);
    assert_eq!(production_metrics.queue_monitoring.queue_operations_total, 4);
    
    // Verify calculated metrics
    assert!(production_metrics.queue_monitoring.queue_saturation_rate_percent > 0.0);
    assert!(production_metrics.queue_monitoring.backpressure_duration_ms > 0.0);
    assert!(production_metrics.queue_monitoring.average_queue_utilization_percent > 0.0);
}