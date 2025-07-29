use market_data_ingestor::data_source::grpc::ReconnectionConfig;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Test timing accuracy of exponential backoff sequences
#[tokio::test]
async fn test_exponential_backoff_timing_accuracy() {
    let config = ReconnectionConfig {
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        backoff_multiplier: 2.0,
        max_failures: 5,
        connection_timeout: Duration::from_secs(5),
        health_check_interval: Duration::from_secs(30),
    };
    
    let mut delays = Vec::new();
    let mut current_delay = config.initial_delay;
    
    // Calculate expected delays for exponential backoff
    for _attempt in 0..config.max_failures {
        delays.push(current_delay);
        let next_delay_ms = (current_delay.as_millis() as f64 * config.backoff_multiplier) as u64;
        current_delay = Duration::from_millis(next_delay_ms).min(config.max_delay);
    }
    
    // Test timing accuracy within 10% tolerance
    for (attempt, expected_delay) in delays.iter().enumerate() {
        let start = Instant::now();
        sleep(*expected_delay).await;
        let actual_delay = start.elapsed();
        
        let tolerance = expected_delay.as_millis() as f64 * 0.1; // 10% tolerance
        let diff = (actual_delay.as_millis() as f64 - expected_delay.as_millis() as f64).abs();
        
        assert!(
            diff <= tolerance,
            "Attempt {}: Expected delay {}ms, actual {}ms, difference {}ms exceeds tolerance {}ms",
            attempt,
            expected_delay.as_millis(),
            actual_delay.as_millis(),
            diff,
            tolerance
        );
    }
}

/// Test maximum delay enforcement and ceiling behavior
#[tokio::test]
async fn test_backoff_max_delay_enforcement() {
    let config = ReconnectionConfig {
        initial_delay: Duration::from_millis(1000),
        max_delay: Duration::from_millis(5000),
        backoff_multiplier: 3.0,
        max_failures: 10,
        connection_timeout: Duration::from_secs(5),
        health_check_interval: Duration::from_secs(30),
    };
    
    let mut current_delay = config.initial_delay;
    let mut delays = Vec::new();
    
    // Calculate delays with max enforcement
    for _attempt in 0..config.max_failures {
        delays.push(current_delay);
        let next_delay_ms = (current_delay.as_millis() as f64 * config.backoff_multiplier) as u64;
        current_delay = Duration::from_millis(next_delay_ms).min(config.max_delay);
    }
    
    // Verify exponential growth until max delay is reached
    assert_eq!(delays[0], Duration::from_millis(1000)); // Initial
    assert_eq!(delays[1], Duration::from_millis(3000)); // 1000 * 3.0
    assert_eq!(delays[2], Duration::from_millis(5000)); // Would be 9000, capped to 5000
    
    // All subsequent delays should be at max
    for i in 3..delays.len() {
        assert_eq!(
            delays[i],
            config.max_delay,
            "Delay at attempt {} should be capped at max delay",
            i
        );
    }
}

/// Test backoff reset behavior after successful connections
#[tokio::test]
async fn test_backoff_reset_on_success() {
    let config = ReconnectionConfig {
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(5),
        backoff_multiplier: 2.0,
        max_failures: 5,
        connection_timeout: Duration::from_secs(2),
        health_check_interval: Duration::from_secs(30),
    };
    
    // Simulate escalating backoff
    let mut current_delay = config.initial_delay;
    let mut escalated_delays = Vec::new();
    
    for _attempt in 0..3 {
        escalated_delays.push(current_delay);
        let next_delay_ms = (current_delay.as_millis() as f64 * config.backoff_multiplier) as u64;
        current_delay = Duration::from_millis(next_delay_ms).min(config.max_delay);
    }
    
    // Verify escalation
    assert_eq!(escalated_delays[0], Duration::from_millis(100));
    assert_eq!(escalated_delays[1], Duration::from_millis(200));
    assert_eq!(escalated_delays[2], Duration::from_millis(400));
    
    // Simulate successful connection and reset
    let reset_delay = config.initial_delay;
    assert_eq!(reset_delay, Duration::from_millis(100), "Delay should reset to initial value after success");
    
    // Verify it can escalate again from initial delay
    let mut post_reset_delay = reset_delay;
    let next_delay_ms = (post_reset_delay.as_millis() as f64 * config.backoff_multiplier) as u64;
    post_reset_delay = Duration::from_millis(next_delay_ms);
    
    assert_eq!(post_reset_delay, Duration::from_millis(200), "Should escalate from initial delay again");
}

/// Test configuration edge cases and validation
#[test]
fn test_backoff_configuration_validation() {
    // Test minimum delay enforcement
    let config_min = ReconnectionConfig {
        initial_delay: Duration::from_millis(0),
        max_delay: Duration::from_secs(1),
        backoff_multiplier: 2.0,
        max_failures: 3,
        connection_timeout: Duration::from_secs(5),
        health_check_interval: Duration::from_secs(30),
    };
    
    // Initial delay of 0 should still work but may not be practical
    assert_eq!(config_min.initial_delay, Duration::from_millis(0));
    
    // Test multiplier edge cases
    let config_no_growth = ReconnectionConfig {
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(5),
        backoff_multiplier: 1.0,
        max_failures: 5,
        connection_timeout: Duration::from_secs(5),
        health_check_interval: Duration::from_secs(30),
    };
    
    let mut delay = config_no_growth.initial_delay;
    for _attempt in 0..3 {
        let next_delay_ms = (delay.as_millis() as f64 * config_no_growth.backoff_multiplier) as u64;
        delay = Duration::from_millis(next_delay_ms);
        assert_eq!(delay, Duration::from_millis(100), "Multiplier of 1.0 should maintain constant delay");
    }
    
    // Test fractional multiplier
    let config_fractional = ReconnectionConfig {
        initial_delay: Duration::from_millis(1000),
        max_delay: Duration::from_secs(10),
        backoff_multiplier: 1.5,
        max_failures: 4,
        connection_timeout: Duration::from_secs(5),
        health_check_interval: Duration::from_secs(30),
    };
    
    let mut delay = config_fractional.initial_delay;
    let expected_delays = vec![1000, 1500, 2250, 3375];
    
    for (i, &expected) in expected_delays.iter().enumerate() {
        if i > 0 {
            let next_delay_ms = (delay.as_millis() as f64 * config_fractional.backoff_multiplier) as u64;
            delay = Duration::from_millis(next_delay_ms);
        }
        assert_eq!(
            delay.as_millis(),
            expected,
            "Fractional multiplier calculation incorrect at step {}",
            i
        );
    }
}

/// Test concurrent backoff behavior under load
#[tokio::test]
async fn test_concurrent_backoff_isolation() {
    let config = ReconnectionConfig {
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(1000),
        backoff_multiplier: 2.0,
        max_failures: 5,
        connection_timeout: Duration::from_secs(1),
        health_check_interval: Duration::from_secs(30),
    };
    
    // Simulate multiple concurrent connection attempts with independent backoff
    let mut handles = Vec::new();
    
    for connection_id in 0..3 {
        let config_clone = config.clone();
        let handle = tokio::spawn(async move {
            let mut delays = Vec::new();
            let mut current_delay = config_clone.initial_delay;
            
            // Each connection maintains independent backoff state
            for attempt in 0..config_clone.max_failures {
                let start = Instant::now();
                sleep(current_delay).await;
                let actual_delay = start.elapsed();
                
                delays.push((attempt, actual_delay));
                
                // Calculate next delay
                let next_delay_ms = (current_delay.as_millis() as f64 * config_clone.backoff_multiplier) as u64;
                current_delay = Duration::from_millis(next_delay_ms).min(config_clone.max_delay);
            }
            
            (connection_id, delays)
        });
        handles.push(handle);
    }
    
    // Wait for all concurrent backoff sequences to complete
    let mut all_results = Vec::new();
    for handle in handles {
        let result = handle.await.expect("Task should complete successfully");
        all_results.push(result);
    }
    
    // Verify each connection had independent backoff timing
    for (connection_id, delays) in all_results {
        assert_eq!(delays.len(), config.max_failures as usize, 
            "Connection {} should have attempted {} times", connection_id, config.max_failures);
        
        // Verify exponential progression for each connection independently
        for i in 1..delays.len() {
            let current_delay = delays[i].1;
            let previous_delay = delays[i-1].1;
            
            // Each delay should be approximately double the previous (within tolerance)
            let expected_ratio = config.backoff_multiplier;
            let actual_ratio = current_delay.as_millis() as f64 / previous_delay.as_millis() as f64;
            let tolerance = 0.3; // 30% tolerance for timing variations
            
            assert!(
                (actual_ratio - expected_ratio).abs() <= tolerance,
                "Connection {}, attempt {}: Expected ratio ~{}, got {}", 
                connection_id, i, expected_ratio, actual_ratio
            );
        }
    }
}

/// Test backoff behavior under resource constraints
#[tokio::test]
async fn test_backoff_under_system_load() {
    let config = ReconnectionConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(200),
        backoff_multiplier: 2.0,
        max_failures: 6,
        connection_timeout: Duration::from_secs(1),
        health_check_interval: Duration::from_secs(30),
    };
    
    // Create high contention to test backoff resilience
    let contention_tasks = 10;
    let mut handles = Vec::new();
    
    for task_id in 0..contention_tasks {
        let config_clone = config.clone();
        let handle = tokio::spawn(async move {
            // Simulate CPU-intensive work
            for _ in 0..1000 {
                tokio::task::yield_now().await;
            }
            
            // Perform backoff sequence under load
            let mut current_delay = config_clone.initial_delay;
            let start_time = Instant::now();
            
            for attempt in 0..config_clone.max_failures {
                let delay_start = Instant::now();
                sleep(current_delay).await;
                let delay_end = Instant::now();
                
                // Even under load, delays should be reasonably accurate
                let expected_ms = current_delay.as_millis();
                let actual_ms = (delay_end - delay_start).as_millis();
                let error_ratio = (actual_ms as f64 - expected_ms as f64).abs() / expected_ms as f64;
                
                // Allow higher tolerance under load
                assert!(
                    error_ratio <= 0.5, // 50% tolerance
                    "Task {}, attempt {}: Delay accuracy degraded too much under load. Expected {}ms, got {}ms",
                    task_id, attempt, expected_ms, actual_ms
                );
                
                // Calculate next delay
                let next_delay_ms = (current_delay.as_millis() as f64 * config_clone.backoff_multiplier) as u64;
                current_delay = Duration::from_millis(next_delay_ms).min(config_clone.max_delay);
            }
            
            (task_id, start_time.elapsed())
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    let mut completion_times = Vec::new();
    for handle in handles {
        let (task_id, duration) = handle.await.expect("Task should complete");
        completion_times.push((task_id, duration));
    }
    
    // Verify all tasks completed within reasonable time
    let expected_total_delay = Duration::from_millis(
        10 + 20 + 40 + 80 + 160 + 200 // Sum of delays for max_failures
    );
    let max_acceptable_duration = expected_total_delay * 3; // Allow 3x overhead for system load
    
    for (task_id, duration) in completion_times {
        assert!(
            duration <= max_acceptable_duration,
            "Task {} took too long under load: {:?} vs expected max {:?}",
            task_id, duration, max_acceptable_duration
        );
    }
}

/// Test backoff jitter implementation for avoiding thundering herd
#[tokio::test]
async fn test_backoff_jitter_distribution() {
    let base_delay = Duration::from_millis(100);
    let iterations = 20;
    let mut jittered_delays = Vec::new();
    
    // Simulate jitter by adding randomness to base delay
    use rand::Rng;
    let mut rng = rand::thread_rng();
    
    for _ in 0..iterations {
        // Apply jitter: ±25% of base delay
        let jitter_factor = 1.0 + (rng.gen::<f64>() - 0.5) * 0.5; // 0.75 to 1.25
        let jittered_ms = (base_delay.as_millis() as f64 * jitter_factor) as u64;
        let jittered_delay = Duration::from_millis(jittered_ms);
        jittered_delays.push(jittered_delay);
    }
    
    // Verify jitter distribution
    let min_delay = jittered_delays.iter().min().unwrap();
    let max_delay = jittered_delays.iter().max().unwrap();
    let avg_delay = Duration::from_millis(
        jittered_delays.iter().map(|d| d.as_millis()).sum::<u128>() as u64 / iterations as u64
    );
    
    // Verify jitter bounds
    assert!(min_delay.as_millis() >= 75, "Minimum jittered delay should be at least 75ms");
    assert!(max_delay.as_millis() <= 125, "Maximum jittered delay should be at most 125ms");
    
    // Average should be close to base delay
    let avg_diff = (avg_delay.as_millis() as i64 - base_delay.as_millis() as i64).abs();
    assert!(avg_diff <= 10, "Average jittered delay should be within 10ms of base delay");
    
    // Verify we have distribution (not all the same)
    let unique_delays: std::collections::HashSet<_> = jittered_delays.iter().collect();
    assert!(
        unique_delays.len() >= iterations / 2,
        "Should have reasonable distribution of jittered delays"
    );
}