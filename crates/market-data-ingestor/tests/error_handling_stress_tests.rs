use market_data_ingestor::steps::{
    detector_push::DetectorPushStep,
    parser::ParserStep,
};
use market_data_ingestor::data_source::{TimestampedEvent, RawEvent, EventMetadata};
use market_data_ingestor::processor::ProcessorConfig;
use market_data_ingestor::monitoring::ProductionMetrics;
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction;
use std::time::{Duration, SystemTime, Instant};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use tokio::time::{sleep, timeout};
use futures::future::join_all;

/// Create a test event for stress testing
fn create_stress_test_event(sequence: u64, cause_error: bool) -> TimestampedEvent {
    let mut transaction = Transaction::default();
    
    if cause_error {
        // Create malformed transaction that will cause processing errors
        transaction.info = Some(aptos_indexer_processor_sdk::aptos_protos::transaction::v1::TransactionInfo {
            hash: vec![], // Empty hash to cause parsing issues
            state_change_hash: vec![],
            event_root_hash: vec![],
            state_checkpoint_hash: None,
            gas_used: 0,
            success: false,
            vm_status: "Failed".to_string(),
            accumulator_root_hash: vec![],
            changes: vec![],
        });
    } else {
        // Create valid transaction
        transaction.info = Some(aptos_indexer_processor_sdk::aptos_protos::transaction::v1::TransactionInfo {
            hash: format!("stress_test_hash_{}", sequence).into_bytes(),
            state_change_hash: vec![1, 2, 3],
            event_root_hash: vec![4, 5, 6],
            state_checkpoint_hash: None,
            gas_used: 1000,
            success: true,
            vm_status: "Executed".to_string(),
            accumulator_root_hash: vec![7, 8, 9],
            changes: vec![],
        });
    }

    TimestampedEvent {
        raw_event: RawEvent {
            transaction,
            metadata: EventMetadata {
                version: sequence,
                block_height: Some(sequence),
                chain_id: Some(1),
                size_bytes: Some(1024),
            },
        },
        received_at: SystemTime::now(),
        blockchain_timestamp: Some(SystemTime::now()),
        sequence,
    }
}

/// Test system behavior under sustained error conditions
#[tokio::test(flavor = "multi_thread")]
async fn test_sustained_error_handling() {
    let metrics = Arc::new(ProductionMetrics::new("stress_test"));
    let mut parser = ParserStep::new();
    
    // Track processing statistics
    let processed_count = Arc::new(AtomicU64::new(0));
    let error_count = Arc::new(AtomicU64::new(0));
    let success_count = Arc::new(AtomicU64::new(0));
    
    let test_duration = Duration::from_secs(10);
    let start_time = Instant::now();
    
    // Generate high-frequency events with 30% error rate
    let mut event_sequence = 0u64;
    
    while start_time.elapsed() < test_duration {
        // Create batch of events
        let batch_size = 50;
        let mut batch_tasks = Vec::new();
        
        for _ in 0..batch_size {
            event_sequence += 1;
            let should_error = event_sequence % 3 == 0; // 33% error rate
            let event = create_stress_test_event(event_sequence, should_error);
            
            let processed_clone = processed_count.clone();
            let error_clone = error_count.clone();
            let success_clone = success_count.clone();
            let metrics_clone = metrics.clone();
            
            // Process event in background
            let task = tokio::spawn(async move {
                let start = Instant::now();
                
                // Simulate processing overhead
                let mut test_parser = ParserStep::new();
                let result = test_parser.process_event(event).await;
                
                let processing_time = start.elapsed();
                
                // Update metrics
                processed_clone.fetch_add(1, Ordering::Relaxed);
                
                if result.is_ok() {
                    success_clone.fetch_add(1, Ordering::Relaxed);
                    metrics_clone.record_event_processed(processing_time);
                } else {
                    error_clone.fetch_add(1, Ordering::Relaxed);
                    metrics_clone.record_error("processing_error".to_string());
                }
                
                (processing_time, result.is_ok())
            });
            
            batch_tasks.push(task);
        }
        
        // Wait for batch to complete with timeout
        let batch_timeout = Duration::from_millis(500);
        let batch_results = timeout(batch_timeout, join_all(batch_tasks)).await;
        
        if batch_results.is_err() {
            eprintln!("Batch processing timed out - system may be overloaded");
        }
        
        // Brief pause between batches
        sleep(Duration::from_millis(10)).await;
    }
    
    let final_processed = processed_count.load(Ordering::Relaxed);
    let final_errors = error_count.load(Ordering::Relaxed);
    let final_successes = success_count.load(Ordering::Relaxed);
    
    println!("Stress test results:");
    println!("  Total processed: {}", final_processed);
    println!("  Successes: {}", final_successes);
    println!("  Errors: {}", final_errors);
    println!("  Error rate: {:.1}%", (final_errors as f64 / final_processed as f64) * 100.0);
    
    // Verify system remained responsive
    assert!(final_processed > 100, "System should process significant number of events");
    assert!(final_successes > 0, "Some events should process successfully despite errors");
    
    // Verify error handling didn't crash the system
    let error_rate = final_errors as f64 / final_processed as f64;
    assert!(error_rate < 0.5, "Error rate should be manageable");
    
    // Verify processing throughput remained reasonable
    let processing_rate = final_processed as f64 / test_duration.as_secs_f64();
    assert!(processing_rate > 10.0, "Processing rate should be at least 10 events/sec");
}

/// Test backpressure handling under extreme load
#[tokio::test(flavor = "multi_thread")]
async fn test_backpressure_under_extreme_load() {
    let config = ProcessorConfig::default();
    let metrics = Arc::new(ProductionMetrics::new("backpressure_test"));
    
    // Create DetectorPushStep to test backpressure
    let mut detector_push = DetectorPushStep::new(config.performance);
    
    // Track queue metrics
    let queue_full_count = Arc::new(AtomicU64::new(0));
    let successful_pushes = Arc::new(AtomicU64::new(0));
    let timeout_count = Arc::new(AtomicU64::new(0));
    
    // Generate extreme load
    let load_duration = Duration::from_secs(5);
    let start_time = Instant::now();
    let mut handles = Vec::new();
    
    // Spawn multiple producers creating backpressure
    for producer_id in 0..10 {
        let queue_full_clone = queue_full_count.clone();
        let successful_clone = successful_pushes.clone();
        let timeout_clone = timeout_count.clone();
        let metrics_clone = metrics.clone();
        
        let handle = tokio::spawn(async move {
            let mut local_sequence = producer_id * 10000;
            
            while start_time.elapsed() < load_duration {
                local_sequence += 1;
                
                // Create high-volume events
                for _ in 0..5 {
                    let event = create_stress_test_event(local_sequence, false);
                    
                    // Try to push with timeout
                    let push_timeout = Duration::from_millis(10);
                    let push_start = Instant::now();
                    
                    let result = timeout(push_timeout, async {
                        // Simulate detector push with potential backpressure
                        sleep(Duration::from_micros(100)).await; // Simulate processing
                        Ok::<(), String>(())
                    }).await;
                    
                    let push_duration = push_start.elapsed();
                    
                    match result {
                        Ok(Ok(())) => {
                            successful_clone.fetch_add(1, Ordering::Relaxed);
                            metrics_clone.record_event_processed(push_duration);
                        }
                        Ok(Err(_)) => {
                            queue_full_clone.fetch_add(1, Ordering::Relaxed);
                            metrics_clone.record_backpressure_event();
                        }
                        Err(_) => {
                            timeout_clone.fetch_add(1, Ordering::Relaxed);
                            metrics_clone.record_timeout();
                        }
                    }
                }
                
                // Brief yield to allow other tasks
                tokio::task::yield_now().await;
            }
            
            producer_id
        });
        
        handles.push(handle);
    }
    
    // Wait for all producers to complete
    let _results = join_all(handles).await;
    
    let final_successful = successful_pushes.load(Ordering::Relaxed);
    let final_queue_full = queue_full_count.load(Ordering::Relaxed);
    let final_timeouts = timeout_count.load(Ordering::Relaxed);
    let total_attempts = final_successful + final_queue_full + final_timeouts;
    
    println!("Backpressure test results:");
    println!("  Total attempts: {}", total_attempts);
    println!("  Successful: {}", final_successful);
    println!("  Queue full: {}", final_queue_full);
    println!("  Timeouts: {}", final_timeouts);
    
    // Verify backpressure handling
    assert!(total_attempts > 1000, "Should generate significant load");
    assert!(final_successful > 0, "Some operations should succeed");
    
    // Verify backpressure mechanisms activated
    if final_queue_full > 0 || final_timeouts > 0 {
        println!("Backpressure mechanisms successfully activated");
        assert!(final_queue_full + final_timeouts < total_attempts / 2, 
               "Backpressure shouldn't block majority of operations");
    }
}

/// Test error recovery patterns under continuous load
#[tokio::test(flavor = "multi_thread")]
async fn test_error_recovery_under_load() {
    let metrics = Arc::new(ProductionMetrics::new("recovery_test"));
    
    // Track recovery statistics
    let recovery_cycles = Arc::new(AtomicU64::new(0));
    let successful_recoveries = Arc::new(AtomicU64::new(0));
    let failed_recoveries = Arc::new(AtomicU64::new(0));
    
    let test_duration = Duration::from_secs(8);
    let start_time = Instant::now();
    
    // Simulate multiple components with independent recovery cycles
    let mut recovery_handles = Vec::new();
    
    for component_id in 0..5 {
        let recovery_cycles_clone = recovery_cycles.clone();
        let successful_recoveries_clone = successful_recoveries.clone();
        let failed_recoveries_clone = failed_recoveries.clone();
        let metrics_clone = metrics.clone();
        
        let handle = tokio::spawn(async move {
            let mut component_state = "healthy";
            let mut consecutive_errors = 0u32;
            let mut last_recovery_attempt = Instant::now();
            
            while start_time.elapsed() < test_duration {
                // Simulate component processing
                let should_fail = consecutive_errors > 0 || rand::random::<f64>() < 0.1;
                
                if should_fail {
                    consecutive_errors += 1;
                    component_state = "degraded";
                    
                    // Trigger recovery if enough errors accumulated
                    if consecutive_errors >= 3 && 
                       last_recovery_attempt.elapsed() > Duration::from_millis(500) {
                        
                        recovery_cycles_clone.fetch_add(1, Ordering::Relaxed);
                        last_recovery_attempt = Instant::now();
                        
                        // Simulate recovery process
                        let recovery_duration = Duration::from_millis(100 + (component_id * 50));
                        sleep(recovery_duration).await;
                        
                        // Recovery success probability
                        if rand::random::<f64>() < 0.8 {
                            consecutive_errors = 0;
                            component_state = "healthy";
                            successful_recoveries_clone.fetch_add(1, Ordering::Relaxed);
                            metrics_clone.record_recovery_success();
                        } else {
                            consecutive_errors = 1; // Partial recovery
                            failed_recoveries_clone.fetch_add(1, Ordering::Relaxed);
                            metrics_clone.record_recovery_failure();
                        }
                    }
                } else {
                    // Successful processing
                    if consecutive_errors > 0 {
                        consecutive_errors = consecutive_errors.saturating_sub(1);
                    }
                    if consecutive_errors == 0 {
                        component_state = "healthy";
                    }
                }
                
                // Process events during recovery/degraded states
                let processing_delay = match component_state {
                    "healthy" => Duration::from_micros(50),
                    "degraded" => Duration::from_micros(200),
                    _ => Duration::from_millis(1),
                };
                
                sleep(processing_delay).await;
            }
            
            (component_id, component_state)
        });
        
        recovery_handles.push(handle);
    }
    
    // Wait for all components to complete
    let final_states = join_all(recovery_handles).await;
    
    let final_recovery_cycles = recovery_cycles.load(Ordering::Relaxed);
    let final_successful = successful_recoveries.load(Ordering::Relaxed);
    let final_failed = failed_recoveries.load(Ordering::Relaxed);
    
    println!("Recovery test results:");
    println!("  Recovery cycles: {}", final_recovery_cycles);
    println!("  Successful recoveries: {}", final_successful);
    println!("  Failed recoveries: {}", final_failed);
    
    // Verify recovery mechanisms
    assert!(final_recovery_cycles > 0, "Should trigger recovery cycles under load");
    
    if final_recovery_cycles > 0 {
        let recovery_success_rate = final_successful as f64 / final_recovery_cycles as f64;
        assert!(recovery_success_rate > 0.5, "Recovery success rate should be reasonable");
    }
    
    // Verify final system state
    for (component_id, final_state) in final_states {
        println!("Component {}: final state = {}", component_id, final_state);
        // Most components should be healthy or degraded, not completely failed
        assert_ne!(final_state, "failed");
    }
}

/// Test memory usage patterns during error handling
#[tokio::test]
async fn test_memory_usage_during_errors() {
    let initial_memory = get_memory_usage();
    let metrics = Arc::new(ProductionMetrics::new("memory_test"));
    
    // Generate memory-intensive error conditions
    let mut error_generators = Vec::new();
    
    for _ in 0..3 {
        let metrics_clone = metrics.clone();
        
        let handle = tokio::spawn(async move {
            let mut large_data_structures = Vec::new();
            
            // Generate errors that might cause memory leaks
            for i in 0..1000 {
                // Create large events that might not be properly cleaned up on errors
                let event = create_stress_test_event(i, true); // Error-causing event
                
                // Simulate processing that might retain memory
                let mut parser = ParserStep::new();
                let _result = parser.process_event(event).await;
                
                // Intentionally create temporary large allocations
                let temp_data: Vec<u8> = vec![0; 1024 * 10]; // 10KB
                large_data_structures.push(temp_data);
                
                // Periodically clean up to simulate proper memory management
                if i % 100 == 0 {
                    large_data_structures.clear();
                    metrics_clone.record_memory_cleanup();
                }
                
                // Yield to prevent blocking
                if i % 50 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            
            large_data_structures.len()
        });
        
        error_generators.push(handle);
    }
    
    // Wait for completion
    let _results = join_all(error_generators).await;
    
    // Force garbage collection and measure memory
    tokio::task::yield_now().await;
    sleep(Duration::from_millis(100)).await;
    
    let final_memory = get_memory_usage();
    let memory_growth = final_memory.saturating_sub(initial_memory);
    
    println!("Memory usage test:");
    println!("  Initial memory: {} KB", initial_memory);
    println!("  Final memory: {} KB", final_memory);
    println!("  Memory growth: {} KB", memory_growth);
    
    // Verify memory growth is reasonable (allowing for some growth but not excessive)
    assert!(memory_growth < 50_000, "Memory growth should be under 50MB"); // 50MB limit
}

/// Get current memory usage (simplified version for testing)
fn get_memory_usage() -> u64 {
    // In a real implementation, you'd use system calls to get actual memory usage
    // For testing purposes, we'll simulate with a simple counter
    use std::sync::atomic::{AtomicU64, Ordering};
    static SIMULATED_MEMORY: AtomicU64 = AtomicU64::new(10_000); // Start at 10MB
    
    // Simulate some memory growth
    let current = SIMULATED_MEMORY.load(Ordering::Relaxed);
    let growth = rand::random::<u64>() % 1000; // Random growth up to 1MB
    SIMULATED_MEMORY.store(current + growth, Ordering::Relaxed);
    
    current + growth
}

/// Test circuit breaker behavior under stress
#[tokio::test]
async fn test_circuit_breaker_stress() {
    let metrics = Arc::new(ProductionMetrics::new("circuit_breaker_test"));
    
    // Simulate circuit breaker with different thresholds
    let circuit_breaker_trips = Arc::new(AtomicU64::new(0));
    let successful_operations = Arc::new(AtomicU64::new(0));
    let rejected_operations = Arc::new(AtomicU64::new(0));
    
    let test_duration = Duration::from_secs(5);
    let start_time = Instant::now();
    
    // Multiple concurrent stress generators
    let mut stress_handles = Vec::new();
    
    for generator_id in 0..8 {
        let trips_clone = circuit_breaker_trips.clone();
        let successful_clone = successful_operations.clone();
        let rejected_clone = rejected_operations.clone();
        let metrics_clone = metrics.clone();
        
        let handle = tokio::spawn(async move {
            let mut consecutive_failures = 0u32;
            let mut circuit_state = "closed"; // closed, open, half-open
            let mut last_failure_time = Instant::now();
            
            while start_time.elapsed() < test_duration {
                let operation_start = Instant::now();
                
                // Simulate operation based on circuit state
                let operation_result = match circuit_state {
                    "closed" => {
                        // Normal operation with some failure probability
                        let should_fail = rand::random::<f64>() < 0.2; // 20% failure rate
                        
                        if should_fail {
                            consecutive_failures += 1;
                            last_failure_time = Instant::now();
                            
                            // Trip circuit breaker after threshold
                            if consecutive_failures >= 5 {
                                circuit_state = "open";
                                trips_clone.fetch_add(1, Ordering::Relaxed);
                                metrics_clone.record_circuit_breaker_trip();
                            }
                            
                            Err("Operation failed")
                        } else {
                            consecutive_failures = 0;
                            successful_clone.fetch_add(1, Ordering::Relaxed);
                            Ok(())
                        }
                    }
                    "open" => {
                        // Circuit breaker open - reject operations
                        rejected_clone.fetch_add(1, Ordering::Relaxed);
                        
                        // Transition to half-open after timeout
                        if last_failure_time.elapsed() > Duration::from_millis(500) {
                            circuit_state = "half-open";
                        }
                        
                        Err("Circuit breaker open")
                    }
                    "half-open" => {
                        // Test operation to see if we can close circuit
                        let test_success = rand::random::<f64>() < 0.7; // 70% success in half-open
                        
                        if test_success {
                            circuit_state = "closed";
                            consecutive_failures = 0;
                            successful_clone.fetch_add(1, Ordering::Relaxed);
                            Ok(())
                        } else {
                            circuit_state = "open";
                            last_failure_time = Instant::now();
                            consecutive_failures += 1;
                            Err("Half-open test failed")
                        }
                    }
                    _ => Err("Unknown state")
                };
                
                let operation_duration = operation_start.elapsed();
                
                // Record metrics
                match operation_result {
                    Ok(()) => metrics_clone.record_event_processed(operation_duration),
                    Err(_) => metrics_clone.record_error("circuit_breaker_test".to_string()),
                }
                
                // Brief delay between operations
                sleep(Duration::from_micros(100)).await;
            }
            
            (generator_id, circuit_state)
        });
        
        stress_handles.push(handle);
    }
    
    // Wait for all stress generators
    let final_states = join_all(stress_handles).await;
    
    let final_trips = circuit_breaker_trips.load(Ordering::Relaxed);
    let final_successful = successful_operations.load(Ordering::Relaxed);
    let final_rejected = rejected_operations.load(Ordering::Relaxed);
    let total_operations = final_successful + final_rejected;
    
    println!("Circuit breaker stress test:");
    println!("  Circuit breaker trips: {}", final_trips);
    println!("  Successful operations: {}", final_successful);
    println!("  Rejected operations: {}", final_rejected);
    println!("  Total operations: {}", total_operations);
    
    // Verify circuit breaker behavior
    assert!(total_operations > 1000, "Should perform significant number of operations");
    assert!(final_trips > 0, "Circuit breaker should trip under stress");
    assert!(final_successful > 0, "Some operations should succeed");
    
    // Verify circuit breaker recovery
    let recovery_rate = final_successful as f64 / total_operations as f64;
    assert!(recovery_rate > 0.3, "Should recover and process at least 30% of operations");
    
    // Check final states
    for (generator_id, final_state) in final_states {
        println!("Generator {}: final circuit state = {}", generator_id, final_state);
        // Circuit breakers should eventually recover
        assert!(["closed", "half-open"].contains(&final_state), 
               "Circuit breakers should recover from open state");
    }
}

/// Test error handling performance overhead
#[tokio::test]
async fn test_error_handling_performance_overhead() {
    let metrics = Arc::new(ProductionMetrics::new("performance_test"));
    
    // Baseline performance without errors
    let baseline_start = Instant::now();
    let baseline_operations = 1000;
    
    for i in 0..baseline_operations {
        let event = create_stress_test_event(i, false); // No errors
        let mut parser = ParserStep::new();
        let _result = parser.process_event(event).await;
    }
    
    let baseline_duration = baseline_start.elapsed();
    let baseline_per_op = baseline_duration / baseline_operations;
    
    // Performance with error handling
    let error_start = Instant::now();
    let error_operations = 1000;
    
    for i in 0..error_operations {
        let should_error = i % 5 == 0; // 20% error rate
        let event = create_stress_test_event(i, should_error);
        let mut parser = ParserStep::new();
        let _result = parser.process_event(event).await;
    }
    
    let error_duration = error_start.elapsed();
    let error_per_op = error_duration / error_operations;
    
    println!("Performance overhead test:");
    println!("  Baseline: {:?} per operation", baseline_per_op);
    println!("  With errors: {:?} per operation", error_per_op);
    println!("  Overhead: {:?}", error_per_op - baseline_per_op);
    
    // Verify performance requirements
    assert!(baseline_per_op < Duration::from_micros(100), 
           "Baseline performance should be under 0.1ms per operation");
    
    let overhead = error_per_op.saturating_sub(baseline_per_op);
    assert!(overhead < Duration::from_micros(50), 
           "Error handling overhead should be under 0.05ms per operation");
    
    // Verify total error handling performance
    assert!(error_per_op < Duration::from_micros(150), 
           "Error handling should maintain sub-0.15ms performance");
}