use market_data_ingestor::steps::detector_push::{
    BackpressureConfig, CircuitBreakerState, DetectorPushStep
};
use common::types::DetectorMessage;
use chrono::Utc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;

/// Test channel timeout behavior under various loads
#[tokio::test]
async fn test_channel_timeout_under_load() {
    let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(10);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(100),
        circuit_breaker_failure_threshold: 5,
        circuit_breaker_recovery_timeout: Duration::from_millis(1000),
        queue_depth_warning_threshold: 0.8,
        retry_attempts: 2,
        retry_base_delay: Duration::from_millis(10),
        retry_max_delay: Duration::from_millis(100),
    };
    
    let detector_push = DetectorPushStep::new(sender, config);
    
    // Fill the channel to capacity to create backpressure
    for i in 0..10 {
        let msg = DetectorMessage::BlockStart {
            block_number: i,
            timestamp: Utc::now(),
        };
        detector_push.push(msg).await.expect("Initial sends should succeed");
    }
    
    // Now attempt to send more messages - these should timeout
    let timeout_start = Instant::now();
    let msg = DetectorMessage::BlockStart {
        block_number: 100,
        timestamp: Utc::now(),
    };
    
    let result = detector_push.push_with_timeout_and_retry(msg).await;
    let timeout_duration = timeout_start.elapsed();
    
    // Should fail due to timeout and retries
    assert!(result.is_err(), "Send should fail due to timeout");
    
    // Should take approximately timeout * retry_attempts
    let expected_min_duration = Duration::from_millis(100 * 3); // 100ms timeout * 3 attempts
    assert!(
        timeout_duration >= expected_min_duration,
        "Timeout duration {:?} should be at least {:?}",
        timeout_duration,
        expected_min_duration
    );
    
    // Drain the channel to restore normal operation
    for _ in 0..5 {
        receiver.recv().await.expect("Should receive messages");
    }
    
    // Verify normal operation is restored
    let msg = DetectorMessage::BlockEnd { block_number: 101 };
    let result = detector_push.push(msg).await;
    assert!(result.is_ok(), "Normal operation should be restored");
}

/// Test circuit breaker activation and recovery cycles
#[tokio::test]
async fn test_circuit_breaker_activation_and_recovery() {
    let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(5);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(50),
        circuit_breaker_failure_threshold: 3,
        circuit_breaker_recovery_timeout: Duration::from_millis(200),
        queue_depth_warning_threshold: 0.6,
        retry_attempts: 1,
        retry_base_delay: Duration::from_millis(10),
        retry_max_delay: Duration::from_millis(50),
    };
    
    let detector_push = DetectorPushStep::new(sender, config);
    
    // Fill channel to create failures
    for i in 0..5 {
        let msg = DetectorMessage::BlockStart {
            block_number: i,
            timestamp: Utc::now(),
        };
        detector_push.push(msg).await.expect("Initial sends should succeed");
    }
    
    // Verify circuit breaker is initially closed
    assert_eq!(detector_push.get_circuit_breaker_state(), CircuitBreakerState::Closed);
    
    // Generate failures to trigger circuit breaker
    for i in 0..5 {
        let msg = DetectorMessage::BlockStart {
            block_number: 100 + i,
            timestamp: Utc::now(),
        };
        let _ = detector_push.push_with_timeout_and_retry(msg).await;
        
        // Check if circuit breaker opened
        if detector_push.get_circuit_breaker_state() == CircuitBreakerState::Open {
            break;
        }
    }
    
    // Circuit breaker should be open after failures
    assert_eq!(detector_push.get_circuit_breaker_state(), CircuitBreakerState::Open);
    
    // Immediate requests should be blocked
    let msg = DetectorMessage::BlockEnd { block_number: 200 };
    let blocked_result = detector_push.push_with_timeout_and_retry(msg).await;
    assert!(blocked_result.is_err(), "Requests should be blocked when circuit breaker is open");
    
    // Wait for recovery timeout
    sleep(Duration::from_millis(250)).await;
    
    // Drain some messages to allow recovery
    for _ in 0..3 {
        receiver.recv().await.expect("Should receive messages");
    }
    
    // Circuit breaker should transition to half-open and then closed
    let msg = DetectorMessage::BlockStart {
        block_number: 300,
        timestamp: Utc::now(),
    };
    let recovery_result = detector_push.push_with_timeout_and_retry(msg).await;
    
    // Should succeed and circuit breaker should be in recovery mode
    if recovery_result.is_ok() {
        // After successful operations, circuit breaker should close
        for i in 0..3 {
            let msg = DetectorMessage::BlockEnd { block_number: 300 + i };
            detector_push.push(msg).await.expect("Recovery operations should succeed");
        }
        
        // Circuit breaker should be closed after recovery
        assert_eq!(detector_push.get_circuit_breaker_state(), CircuitBreakerState::Closed);
    }
}

/// Test queue depth monitoring accuracy
#[tokio::test]
async fn test_queue_depth_monitoring_accuracy() {
    let channel_capacity = 8;
    let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(channel_capacity);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(100),
        circuit_breaker_failure_threshold: 10,
        circuit_breaker_recovery_timeout: Duration::from_secs(1),
        queue_depth_warning_threshold: 0.75,
        retry_attempts: 1,
        retry_base_delay: Duration::from_millis(10),
        retry_max_delay: Duration::from_millis(100),
    };
    
    let detector_push = DetectorPushStep::new(sender, config);
    
    // Send messages and monitor queue depth
    let mut queue_depths = Vec::new();
    
    for i in 0..channel_capacity {
        let msg = DetectorMessage::BlockStart {
            block_number: i as u64,
            timestamp: Utc::now(),
        };
        
        let metrics = detector_push.push_with_metrics(msg).await
            .expect("Send should succeed");
        
        queue_depths.push(metrics.queue_depth_before);
        
        // Verify queue depth increases
        assert_eq!(
            metrics.queue_depth_before,
            i,
            "Queue depth should be {} before sending message {}",
            i,
            i
        );
        
        // Verify channel capacity is accurately reported
        assert_eq!(
            metrics.channel_capacity,
            channel_capacity,
            "Channel capacity should be accurately reported"
        );
    }
    
    // At this point, queue should be full
    let final_metrics = detector_push.get_congestion_metrics();
    assert_eq!(
        final_metrics.queue_depth_current,
        channel_capacity,
        "Queue should be at capacity"
    );
    
    // Drain half the messages
    for _ in 0..(channel_capacity / 2) {
        receiver.recv().await.expect("Should receive message");
    }
    
    // Send another message to verify queue depth tracking
    let msg = DetectorMessage::BlockEnd { block_number: 100 };
    let post_drain_metrics = detector_push.push_with_metrics(msg).await
        .expect("Send should succeed after draining");
    
    // Queue depth should reflect the drained messages
    let expected_depth = channel_capacity / 2;
    assert_eq!(
        post_drain_metrics.queue_depth_before,
        expected_depth,
        "Queue depth should be {} after draining {} messages",
        expected_depth,
        channel_capacity / 2
    );
}

/// Test graceful degradation during congestion
#[tokio::test]
async fn test_graceful_degradation_during_congestion() {
    let (sender, _receiver) = mpsc::channel::<DetectorMessage>(3);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(50),
        circuit_breaker_failure_threshold: 2,
        circuit_breaker_recovery_timeout: Duration::from_millis(100),
        queue_depth_warning_threshold: 0.66, // 2/3 capacity
        retry_attempts: 2,
        retry_base_delay: Duration::from_millis(5),
        retry_max_delay: Duration::from_millis(25),
    };
    
    let detector_push = DetectorPushStep::new(sender, config);
    
    // Fill channel to trigger congestion warnings
    let mut congestion_detected = false;
    let mut backpressure_detected = false;
    
    for i in 0..5 {
        let msg = DetectorMessage::BlockStart {
            block_number: i,
            timestamp: Utc::now(),
        };
        
        let start_time = Instant::now();
        let result = detector_push.push_with_timeout_and_retry(msg).await;
        let operation_time = start_time.elapsed();
        
        // Check if backpressure is detected
        if detector_push.is_experiencing_backpressure() {
            backpressure_detected = true;
        }
        
        // Check congestion metrics
        let metrics = detector_push.get_congestion_metrics();
        if metrics.channel_congestion_events > 0 {
            congestion_detected = true;
        }
        
        // First few messages should succeed
        if i < 3 {
            assert!(result.is_ok(), "Initial messages should succeed");
        } else {
            // Later messages may fail due to congestion
            if result.is_err() {
                // Verify it's a timeout/congestion error, not a different error
                let error_msg = result.unwrap_err().to_string();
                assert!(
                    error_msg.contains("timeout") || 
                    error_msg.contains("Circuit breaker") ||
                    error_msg.contains("retry"),
                    "Error should be related to congestion: {}",
                    error_msg
                );
            }
        }
        
        // Operations under congestion should take longer
        if i >= 2 {
            assert!(
                operation_time >= Duration::from_millis(40),
                "Congested operations should take longer: {:?}",
                operation_time
            );
        }
    }
    
    // Verify graceful degradation was detected
    assert!(
        congestion_detected || backpressure_detected,
        "System should detect congestion or backpressure"
    );
    
    // Verify circuit breaker state progression
    let cb_state = detector_push.get_circuit_breaker_state();
    assert!(
        cb_state == CircuitBreakerState::Open || cb_state == CircuitBreakerState::HalfOpen,
        "Circuit breaker should be activated under sustained congestion"
    );
}

/// Test performance impact of backpressure handling
#[tokio::test]
async fn test_backpressure_handling_performance_impact() {
    let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(100);
    
    // Test with minimal backpressure configuration for performance
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(1000),
        circuit_breaker_failure_threshold: 50,
        circuit_breaker_recovery_timeout: Duration::from_secs(1),
        queue_depth_warning_threshold: 0.9,
        retry_attempts: 1,
        retry_base_delay: Duration::from_millis(1),
        retry_max_delay: Duration::from_millis(10),
    };
    
    let detector_push = DetectorPushStep::new(sender, config);
    
    // Measure baseline performance with normal operations
    let num_operations = 50;
    let start_time = Instant::now();
    
    for i in 0..num_operations {
        let msg = DetectorMessage::BlockStart {
            block_number: i,
            timestamp: Utc::now(),
        };
        
        detector_push.push_with_metrics(msg).await
            .expect("Normal operations should succeed");
    }
    
    let total_duration = start_time.elapsed();
    let avg_operation_time = total_duration / num_operations as u32;
    
    // Verify low overhead for normal operations
    assert!(
        avg_operation_time <= Duration::from_millis(5),
        "Average operation time should be low: {:?}",
        avg_operation_time
    );
    
    // Drain messages to maintain performance
    let drain_start = Instant::now();
    for _ in 0..num_operations {
        receiver.recv().await.expect("Should receive messages");
    }
    let drain_time = drain_start.elapsed();
    
    // Verify draining doesn't block significantly
    assert!(
        drain_time <= Duration::from_millis(100),
        "Draining should be fast: {:?}",
        drain_time
    );
    
    // Test performance under light congestion
    let congestion_start = Instant::now();
    
    // Send without draining to create mild backpressure
    for i in 0..20 {
        let msg = DetectorMessage::BlockEnd { block_number: i };
        let result = detector_push.push_with_timeout_and_retry(msg).await;
        
        // Should still succeed but may take longer
        if result.is_err() {
            break; // Stop if we hit actual failures
        }
    }
    
    let congestion_duration = congestion_start.elapsed();
    let avg_congestion_time = congestion_duration / 20;
    
    // Even under light congestion, performance shouldn't degrade too much
    assert!(
        avg_congestion_time <= Duration::from_millis(50),
        "Performance under light congestion should be acceptable: {:?}",
        avg_congestion_time
    );
    
    // Verify backpressure detection overhead is minimal
    let metrics_start = Instant::now();
    let _metrics = detector_push.get_congestion_metrics();
    let _is_backpressure = detector_push.is_experiencing_backpressure();
    let _cb_state = detector_push.get_circuit_breaker_state();
    let metrics_time = metrics_start.elapsed();
    
    assert!(
        metrics_time <= Duration::from_millis(1),
        "Metrics collection should be very fast: {:?}",
        metrics_time
    );
}

/// Test exponential backoff timing in retry scenarios
#[tokio::test]
async fn test_retry_exponential_backoff_timing() {
    let (sender, _receiver) = mpsc::channel::<DetectorMessage>(1);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(10), // Very short to force timeouts
        circuit_breaker_failure_threshold: 10,
        circuit_breaker_recovery_timeout: Duration::from_secs(10),
        queue_depth_warning_threshold: 0.5,
        retry_attempts: 4,
        retry_base_delay: Duration::from_millis(20),
        retry_max_delay: Duration::from_millis(200),
    };
    
    let detector_push = DetectorPushStep::new(sender, config.clone());
    
    // Fill the channel to guarantee timeout
    let fill_msg = DetectorMessage::BlockStart {
        block_number: 0,
        timestamp: Utc::now(),
    };
    detector_push.push(fill_msg).await.expect("First send should succeed");
    
    // Attempt operation that will retry with exponential backoff
    let start_time = Instant::now();
    let msg = DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    };
    
    let result = detector_push.push_with_timeout_and_retry(msg).await;
    let total_time = start_time.elapsed();
    
    // Should fail after all retries
    assert!(result.is_err(), "Should fail after all retries");
    
    // Calculate expected minimum time based on exponential backoff
    // timeout * attempts + backoff_delays
    let timeout_time = config.send_timeout * (config.retry_attempts + 1);
    let backoff_delays = Duration::from_millis(20 + 40 + 80 + 160); // Exponential progression
    let expected_min_time = timeout_time + backoff_delays;
    
    assert!(
        total_time >= expected_min_time * 8 / 10, // Allow 20% tolerance
        "Total time {:?} should be at least 80% of expected minimum {:?}",
        total_time,
        expected_min_time
    );
    
    // Should not take excessively longer than expected
    let expected_max_time = expected_min_time * 2;
    assert!(
        total_time <= expected_max_time,
        "Total time {:?} should not exceed twice expected maximum {:?}",
        total_time,
        expected_max_time
    );
}

/// Test concurrent backpressure handling with multiple senders
#[tokio::test]
async fn test_concurrent_backpressure_handling() {
    let (sender, mut receiver) = mpsc::channel::<DetectorMessage>(5);
    
    let config = BackpressureConfig {
        send_timeout: Duration::from_millis(100),
        circuit_breaker_failure_threshold: 3,
        circuit_breaker_recovery_timeout: Duration::from_millis(500),
        queue_depth_warning_threshold: 0.6,
        retry_attempts: 2,
        retry_base_delay: Duration::from_millis(10),
        retry_max_delay: Duration::from_millis(50),
    };
    
    // Create multiple detector push instances (simulating concurrent usage)
    let num_concurrent = 3;
    let mut handles = Vec::new();
    
    for thread_id in 0..num_concurrent {
        let sender_clone = sender.clone();
        let config_clone = config.clone();
        
        let handle = tokio::spawn(async move {
            let detector_push = DetectorPushStep::new(sender_clone, config_clone);
            let mut results = Vec::new();
            
            // Each thread sends multiple messages
            for msg_id in 0..5 {
                let msg = DetectorMessage::BlockStart {
                    block_number: (thread_id * 100 + msg_id) as u64,
                    timestamp: Utc::now(),
                };
                
                let start = Instant::now();
                let result = detector_push.push_with_timeout_and_retry(msg).await;
                let duration = start.elapsed();
                
                results.push((msg_id, result.is_ok(), duration));
                
                // Small delay between messages
                sleep(Duration::from_millis(10)).await;
            }
            
            (thread_id, results, detector_push.get_congestion_metrics())
        });
        
        handles.push(handle);
    }
    
    // Slowly drain messages to create controlled backpressure
    let drain_handle = tokio::spawn(async move {
        for _ in 0..15 {
            sleep(Duration::from_millis(50)).await;
            if let Ok(_msg) = receiver.try_recv() {
                // Message drained
            }
        }
    });
    
    // Wait for all senders to complete
    let mut all_results = Vec::new();
    for handle in handles {
        let result = handle.await.expect("Task should complete");
        all_results.push(result);
    }
    
    drain_handle.await.expect("Drain task should complete");
    
    // Analyze results
    let mut total_successes = 0;
    let mut total_attempts = 0;
    let mut max_congestion_events = 0;
    
    for (thread_id, results, congestion_metrics) in all_results {
        let successes = results.iter().filter(|(_, success, _)| *success).count();
        total_successes += successes;
        total_attempts += results.len();
        max_congestion_events = max_congestion_events.max(congestion_metrics.channel_congestion_events);
        
        println!("Thread {}: {}/{} successful, {} congestion events", 
            thread_id, successes, results.len(), congestion_metrics.channel_congestion_events);
    }
    
    // Verify system handled concurrent load reasonably
    let success_rate = total_successes as f64 / total_attempts as f64;
    assert!(
        success_rate >= 0.3, // At least 30% success rate under congestion
        "Success rate {} should be reasonable under concurrent load",
        success_rate
    );
    
    // Verify congestion was detected
    assert!(
        max_congestion_events > 0,
        "System should detect congestion under concurrent load"
    );
}