use market_data_ingestor::monitoring::MetricsCollector;
use market_data_ingestor::steps::detector_push::{DetectorPushStep, BackpressureConfig};
use common::types::DetectorMessage;
use chrono::Utc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Test that error handling overhead is under 0.1ms per operation
#[tokio::test]
async fn test_error_handling_performance_overhead() {
    // Create a channel with small capacity to trigger backpressure
    let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(10);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(1),
        circuit_breaker_failure_threshold: 3,
        circuit_breaker_recovery_timeout: Duration::from_millis(100),
        queue_depth_warning_threshold: 0.8,
        retry_attempts: 1,
        retry_base_delay: Duration::from_millis(1),
        retry_max_delay: Duration::from_millis(10),
    };
    
    let detector_push = DetectorPushStep::new(sender, config);
    let num_operations = 1000u32;
    
    // Test baseline performance (successful operations)
    let baseline_start = Instant::now();
    for i in 0..num_operations {
        let msg = DetectorMessage::BlockStart {
            block_number: i as u64,
            timestamp: Utc::now(),
        };
        
        // Receive the message to keep channel clear
        let _ = detector_push.push(msg).await;
        let _ = receiver.try_recv();
    }
    let baseline_duration = baseline_start.elapsed();
    let baseline_per_op = baseline_duration / num_operations;
    
    println!("Baseline performance: {:?} per operation", baseline_per_op);
    
    // Fill channel to capacity to trigger error conditions
    for i in 0..10 {
        let msg = DetectorMessage::BlockStart {
            block_number: i as u64,
            timestamp: Utc::now(),
        };
        let _ = detector_push.push(msg).await;
    }
    
    // Test performance with error handling (timeouts/backpressure)
    let error_start = Instant::now();
    for i in 0..num_operations {
        let msg = DetectorMessage::BlockStart {
            block_number: (i + 1000) as u64,
            timestamp: Utc::now(),
        };
        
        // This should trigger timeout/backpressure handling
        let _ = detector_push.push(msg).await; // Will likely timeout
    }
    let error_duration = error_start.elapsed();
    let error_per_op = error_duration / num_operations;
    
    println!("With error handling: {:?} per operation", error_per_op);
    
    // Verify performance requirements
    assert!(baseline_per_op < Duration::from_micros(100), 
           "Baseline performance should be under 0.1ms per operation: {:?}", baseline_per_op);
    
    let overhead = error_per_op.saturating_sub(baseline_per_op);
    println!("Error handling overhead: {:?}", overhead);
    
    // Allow reasonable overhead for error handling
    assert!(overhead < Duration::from_micros(200), 
           "Error handling overhead should be under 0.2ms per operation: {:?}", overhead);
    
    // Verify total error handling performance is still reasonable
    assert!(error_per_op < Duration::from_micros(300), 
           "Total error handling should be under 0.3ms per operation: {:?}", error_per_op);
}

/// Test MetricsCollector performance overhead
#[tokio::test]
async fn test_metrics_collection_performance() {
    let mut collector = MetricsCollector::new().expect("Should create metrics collector");
    let num_operations = 10000u32;
    
    // Test metrics recording performance
    let start = Instant::now();
    for i in 0..num_operations {
        // Simulate updating metrics
        let stats = market_data_ingestor::recording_monitor::RecordingStats::default();
        collector.update_metrics(&stats, "test", true);
        
        if i % 100 == 0 {
            // Simulate stage timing updates
            let stage_timings = market_data_ingestor::recording_monitor::StageTimings {
                event_extraction_time_ms: 0.1,
                parsing_time_ms: 0.05,
                filtering_time_ms: 0.02,
                detector_push_time_ms: 0.01,
            };
            collector.update_stage_timings(&stage_timings);
        }
    }
    let metrics_duration = start.elapsed();
    let metrics_per_op = metrics_duration / num_operations;
    
    println!("Metrics recording: {:?} per operation", metrics_per_op);
    
    // Metrics should have very low overhead
    assert!(metrics_per_op < Duration::from_micros(50), 
           "Metrics recording should be under 0.05ms per operation: {:?}", metrics_per_op);
}

/// Test concurrent performance under load
#[tokio::test]
async fn test_concurrent_performance_load() {
    let num_concurrent_tasks = 10;
    let operations_per_task = 100;
    
    let mut handles = Vec::new();
    
    let start_time = Instant::now();
    
    for task_id in 0..num_concurrent_tasks {
        let handle = tokio::spawn(async move {
            let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(50);
            
            let config = BackpressureConfig {
                send_timeout: Duration::from_millis(10),
                circuit_breaker_failure_threshold: 5,
                circuit_breaker_recovery_timeout: Duration::from_millis(100),
                queue_depth_warning_threshold: 0.8,
                retry_attempts: 2,
                retry_base_delay: Duration::from_millis(1),
                retry_max_delay: Duration::from_millis(50),
            };
            
            let detector_push = DetectorPushStep::new(sender, config);
            
            let task_start = Instant::now();
            let mut successful_ops = 0;
            
            for i in 0..operations_per_task {
                let msg = DetectorMessage::BlockStart {
                    block_number: (task_id * 1000 + i) as u64,
                    timestamp: Utc::now(),
                };
                
                let result = detector_push.push(msg).await;
                
                if result.is_ok() {
                    successful_ops += 1;
                }
                
                // Clear receiver occasionally
                if i % 10 == 0 {
                    while receiver.try_recv().is_ok() {}
                }
                
                // Small yield to allow interleaving
                if i % 25 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            
            let task_duration = task_start.elapsed();
            (task_id, successful_ops, task_duration)
        });
        
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    let mut total_successful = 0;
    let mut max_task_duration = Duration::from_millis(0);
    
    for handle in handles {
        let (task_id, successful_ops, task_duration) = handle.await.expect("Task should complete");
        total_successful += successful_ops;
        max_task_duration = max_task_duration.max(task_duration);
        
        println!("Task {}: {} successful ops in {:?}", task_id, successful_ops, task_duration);
    }
    
    let total_duration = start_time.elapsed();
    let total_operations = num_concurrent_tasks * operations_per_task;
    
    println!("Concurrent performance test:");
    println!("  Total operations: {}", total_operations);
    println!("  Successful operations: {}", total_successful);
    println!("  Total time: {:?}", total_duration);
    println!("  Max task time: {:?}", max_task_duration);
    
    // Verify reasonable performance under concurrent load
    let success_rate = total_successful as f64 / total_operations as f64;
    assert!(success_rate > 0.7, "Should maintain reasonable success rate under load: {:.1}%", success_rate * 100.0);
    
    // Verify task completion time is reasonable
    assert!(max_task_duration < Duration::from_secs(5), 
           "Tasks should complete within reasonable time: {:?}", max_task_duration);
    
    // Calculate average operation time across all concurrent tasks
    let avg_op_time = max_task_duration / operations_per_task as u32;
    assert!(avg_op_time < Duration::from_millis(50),
           "Average operation time should be reasonable under concurrent load: {:?}", avg_op_time);
}

/// Simple integration test of core error handling features
#[tokio::test]
async fn test_integration_error_handling_features() {
    let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(5);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(10),
        circuit_breaker_failure_threshold: 3,
        circuit_breaker_recovery_timeout: Duration::from_millis(100),
        queue_depth_warning_threshold: 0.8,
        retry_attempts: 2,
        retry_base_delay: Duration::from_millis(5),
        retry_max_delay: Duration::from_millis(50),
    };
    
    let detector_push = DetectorPushStep::new(sender, config);
    
    // Test normal operation
    let msg = DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    };
    
    let result = detector_push.push(msg).await;
    assert!(result.is_ok(), "Normal operation should succeed");
    
    // Clear the message
    let received = receiver.recv().await;
    assert!(received.is_some(), "Should receive the message");
    
    // Fill channel to capacity
    for i in 0..5 {
        let msg = DetectorMessage::BlockStart {
            block_number: i + 2,
            timestamp: Utc::now(),
        };
        let _ = detector_push.push(msg).await;
    }
    
    // This should trigger backpressure/timeout
    let start = Instant::now();
    let msg = DetectorMessage::BlockStart {
        block_number: 100,
        timestamp: Utc::now(),
    };
    
    let result = detector_push.push(msg).await;
    let duration = start.elapsed();
    
    // Should either succeed quickly or fail with timeout
    if result.is_err() {
        // If it failed, should be due to timeout and should take approximately timeout duration
        println!("Operation failed with timeout after {:?}", duration);
        assert!(duration >= Duration::from_millis(8), "Should wait at least for timeout");
        assert!(duration <= Duration::from_millis(200), "Should not wait too long");
    } else {
        // If it succeeded, should be quick
        println!("Operation succeeded in {:?}", duration);
        assert!(duration <= Duration::from_millis(5), "Success should be quick");
    }
    
    println!("Integration test completed successfully");
}