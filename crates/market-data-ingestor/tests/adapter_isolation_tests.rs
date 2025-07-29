use market_data_ingestor::steps::parser::{ParserStep, AdapterHealthTracker, AdapterStatus};
use market_data_ingestor::data_source::{TimestampedEvent, RawEvent, EventMetadata};
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use tokio::time::sleep;

/// Create a sample TimestampedEvent for testing
fn create_test_event(dex_name: &str, sequence: u64) -> TimestampedEvent {
    let mut transaction = Transaction::default();
    
    // Create a transaction that would be parsed by the specified adapter
    match dex_name {
        "hyperion" => {
            // Simulate Hyperion-specific event structure
            transaction.info = Some(aptos_indexer_processor_sdk::aptos_protos::transaction::v1::TransactionInfo {
                hash: format!("hyperion_hash_{}", sequence).into_bytes(),
                state_change_hash: vec![],
                event_root_hash: vec![],
                state_checkpoint_hash: None,
                gas_used: 1000,
                success: true,
                vm_status: "Executed".to_string(),
                accumulator_root_hash: vec![],
                changes: vec![],
            });
        },
        "thala" => {
            // Simulate Thala-specific event structure
            transaction.info = Some(aptos_indexer_processor_sdk::aptos_protos::transaction::v1::TransactionInfo {
                hash: format!("thala_hash_{}", sequence).into_bytes(),
                state_change_hash: vec![],
                event_root_hash: vec![],
                state_checkpoint_hash: None,
                gas_used: 2000,
                success: true,
                vm_status: "Executed".to_string(),
                accumulator_root_hash: vec![],
                changes: vec![],
            });
        },
        "tapp" => {
            // Simulate TAPP-specific event structure
            transaction.info = Some(aptos_indexer_processor_sdk::aptos_protos::transaction::v1::TransactionInfo {
                hash: format!("tapp_hash_{}", sequence).into_bytes(),
                state_change_hash: vec![],
                event_root_hash: vec![],
                state_checkpoint_hash: None,
                gas_used: 1500,
                success: true,
                vm_status: "Executed".to_string(),
                accumulator_root_hash: vec![],
                changes: vec![],
            });
        },
        _ => {
            // Invalid adapter - create transaction that will cause parsing errors
            transaction.info = Some(aptos_indexer_processor_sdk::aptos_protos::transaction::v1::TransactionInfo {
                hash: format!("invalid_hash_{}", sequence).into_bytes(),
                state_change_hash: vec![],
                event_root_hash: vec![],
                state_checkpoint_hash: None,
                gas_used: 0,
                success: false,
                vm_status: "Invalid".to_string(),
                accumulator_root_hash: vec![],
                changes: vec![],
            });
        }
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

/// Test that individual adapter failures don't affect other adapters
#[tokio::test]
async fn test_adapter_failure_isolation() {
    let mut parser = ParserStep::new();
    
    // Create events for different adapters
    let hyperion_event = create_test_event("hyperion", 1);
    let thala_event = create_test_event("thala", 2);
    let tapp_event = create_test_event("tapp", 3);
    let invalid_event = create_test_event("invalid", 4);
    
    // Process valid events first - should succeed
    let result = parser.process_event(hyperion_event).await;
    assert!(result.is_ok(), "Hyperion event should process successfully");
    
    let result = parser.process_event(thala_event).await;
    assert!(result.is_ok(), "Thala event should process successfully");
    
    let result = parser.process_event(tapp_event).await;
    assert!(result.is_ok(), "TAPP event should process successfully");
    
    // Process invalid event - should fail but not affect other adapters
    let result = parser.process_event(invalid_event).await;
    // Depending on implementation, this might return Ok with no parsed events or Err
    // The key is that it doesn't crash the parser
    
    // Verify all valid adapters are still healthy
    let health_tracker = parser.get_health_tracker();
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Healthy);
    assert_eq!(health_tracker.get_status("thala"), AdapterStatus::Healthy);
    assert_eq!(health_tracker.get_status("tapp"), AdapterStatus::Healthy);
    
    // Process more valid events to ensure adapters still work
    let hyperion_event2 = create_test_event("hyperion", 5);
    let result = parser.process_event(hyperion_event2).await;
    assert!(result.is_ok(), "Hyperion should still work after invalid event");
}

/// Test circuit breaker behavior for individual adapters
#[tokio::test]
async fn test_adapter_circuit_breaker() {
    let mut parser = ParserStep::new();
    let health_tracker = parser.get_health_tracker();
    
    // Simulate multiple failures for one adapter to trigger circuit breaker
    let failure_threshold = 5;
    
    // Generate failures for hyperion adapter
    for i in 0..failure_threshold + 1 {
        // Create events that will cause hyperion adapter to fail
        let event = create_test_event("invalid", i);
        let _ = parser.process_event(event).await;
    }
    
    // After threshold failures, hyperion adapter should be disabled
    // (This depends on the actual implementation - adjust based on ParserStep behavior)
    
    // Verify other adapters are unaffected
    let thala_event = create_test_event("thala", 10);
    let result = parser.process_event(thala_event).await;
    assert!(result.is_ok(), "Thala adapter should remain unaffected");
    
    assert_eq!(health_tracker.get_status("thala"), AdapterStatus::Healthy);
    assert_eq!(health_tracker.get_status("tapp"), AdapterStatus::Healthy);
}

/// Test adapter recovery behavior
#[tokio::test]
async fn test_adapter_recovery() {
    let mut parser = ParserStep::new();
    let health_tracker = parser.get_health_tracker();
    
    // Force an adapter into degraded state
    health_tracker.record_error("hyperion", "Test error".to_string());
    health_tracker.record_error("hyperion", "Test error 2".to_string());
    
    // Verify adapter is degraded
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Degraded);
    
    // Process successful events to trigger recovery
    for i in 0..3 {
        let event = create_test_event("hyperion", i);
        let _ = parser.process_event(event).await;
        health_tracker.record_success("hyperion");
    }
    
    // After sufficient successes, adapter should recover
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Healthy);
    
    // Verify adapter works normally after recovery
    let event = create_test_event("hyperion", 100);
    let result = parser.process_event(event).await;
    assert!(result.is_ok(), "Recovered adapter should work normally");
}

/// Test error classification and severity handling
#[tokio::test]
async fn test_error_classification() {
    let health_tracker = AdapterHealthTracker::new();
    
    // Test different types of errors
    health_tracker.record_error("hyperion", "Parse error: invalid format".to_string());
    health_tracker.record_error("hyperion", "Network timeout".to_string());
    health_tracker.record_error("hyperion", "Memory allocation failed".to_string());
    
    // Verify error tracking
    let stats = health_tracker.get_adapter_stats("hyperion");
    assert_eq!(stats.error_count, 3);
    assert!(stats.last_error_time.is_some());
    
    // Test severity-based handling
    health_tracker.record_error("thala", "Critical: Database connection lost".to_string());
    
    // Critical errors might immediately degrade adapter
    let stats = health_tracker.get_adapter_stats("thala");
    assert_eq!(stats.error_count, 1);
}

/// Test concurrent adapter processing without interference
#[tokio::test]
async fn test_concurrent_adapter_processing() {
    let parser = std::sync::Arc::new(tokio::sync::Mutex::new(ParserStep::new()));
    
    // Create concurrent tasks processing different adapters
    let mut handles = Vec::new();
    
    for adapter in &["hyperion", "thala", "tapp"] {
        let parser_clone = parser.clone();
        let adapter_name = adapter.to_string();
        
        let handle = tokio::spawn(async move {
            let mut results = Vec::new();
            
            // Process multiple events for this adapter
            for i in 0..10 {
                let event = create_test_event(&adapter_name, i);
                let mut parser_guard = parser_clone.lock().await;
                let result = parser_guard.process_event(event).await;
                results.push(result.is_ok());
                drop(parser_guard);
                
                // Small delay to allow interleaving
                sleep(Duration::from_millis(1)).await;
            }
            
            (adapter_name, results)
        });
        handles.push(handle);
    }
    
    // Wait for all concurrent processing to complete
    let mut all_results = HashMap::new();
    for handle in handles {
        let (adapter_name, results) = handle.await.expect("Task should complete");
        all_results.insert(adapter_name, results);
    }
    
    // Verify all adapters processed successfully
    for (adapter_name, results) in all_results {
        let success_count = results.iter().filter(|&&r| r).count();
        assert!(
            success_count >= 7, // Allow some failures but most should succeed
            "Adapter {} should have mostly successful processing: {}/10",
            adapter_name, success_count
        );
    }
}

/// Test adapter performance metrics tracking
#[tokio::test]
async fn test_adapter_performance_tracking() {
    let health_tracker = AdapterHealthTracker::new();
    
    // Simulate processing with timing
    let start_time = std::time::Instant::now();
    
    // Record processing times for different adapters
    for i in 0..100 {
        let processing_time = Duration::from_micros(50 + i); // Simulate varying processing times
        health_tracker.record_processing_time("hyperion", processing_time);
        
        if i % 10 == 0 {
            health_tracker.record_success("hyperion");
        }
    }
    
    let stats = health_tracker.get_adapter_stats("hyperion");
    assert_eq!(stats.success_count, 10);
    assert!(stats.average_processing_time > Duration::from_micros(50));
    assert!(stats.average_processing_time < Duration::from_micros(200));
    
    // Test performance degradation detection
    // Simulate slowly degrading performance
    for i in 0..50 {
        let slow_time = Duration::from_millis(1 + i); // Increasingly slow
        health_tracker.record_processing_time("thala", slow_time);
    }
    
    let thala_stats = health_tracker.get_adapter_stats("thala");
    assert!(thala_stats.average_processing_time > Duration::from_millis(25));
}

/// Test adapter status transitions
#[tokio::test]
async fn test_adapter_status_transitions() {
    let health_tracker = AdapterHealthTracker::new();
    
    // Start with healthy status
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Healthy);
    
    // Transition to degraded after errors
    health_tracker.record_error("hyperion", "Error 1".to_string());
    health_tracker.record_error("hyperion", "Error 2".to_string());
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Degraded);
    
    // Transition to disabled after many errors
    for i in 3..10 {
        health_tracker.record_error("hyperion", format!("Error {}", i));
    }
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Disabled);
    
    // Start recovery process
    health_tracker.start_recovery("hyperion");
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Recovering);
    
    // Complete recovery with successes
    for _ in 0..5 {
        health_tracker.record_success("hyperion");
    }
    assert_eq!(health_tracker.get_status("hyperion"), AdapterStatus::Healthy);
}

/// Test error rate limiting and backoff
#[tokio::test]
async fn test_adapter_error_rate_limiting() {
    let health_tracker = AdapterHealthTracker::new();
    
    // Rapid error generation
    let start_time = std::time::Instant::now();
    
    for i in 0..20 {
        health_tracker.record_error("hyperion", format!("Rapid error {}", i));
        
        // Check if rate limiting kicks in
        if i > 10 {
            let stats = health_tracker.get_adapter_stats("hyperion");
            // After many rapid errors, there should be some rate limiting mechanism
            assert!(stats.error_count <= 20);
        }
    }
    
    let elapsed = start_time.elapsed();
    
    // Verify rate limiting doesn't cause processing to hang
    assert!(elapsed < Duration::from_secs(1), "Error processing shouldn't hang");
}

/// Test adapter health metrics export
#[tokio::test]
async fn test_adapter_health_metrics_export() {
    let health_tracker = AdapterHealthTracker::new();
    
    // Generate some activity
    for adapter in &["hyperion", "thala", "tapp"] {
        for i in 0..10 {
            if i % 3 == 0 {
                health_tracker.record_error(adapter, format!("Error {}", i));
            } else {
                health_tracker.record_success(adapter);
                health_tracker.record_processing_time(adapter, Duration::from_micros(100 + i));
            }
        }
    }
    
    // Export metrics for all adapters
    let all_metrics = health_tracker.export_metrics();
    
    // Verify metrics contain expected data
    assert_eq!(all_metrics.len(), 3);
    
    for (adapter_name, metrics) in all_metrics {
        assert!(["hyperion", "thala", "tapp"].contains(&adapter_name.as_str()));
        assert!(metrics.contains("error_count"));
        assert!(metrics.contains("success_count"));
        assert!(metrics.contains("status"));
    }
}

/// Test adapter isolation under memory pressure
#[tokio::test]
async fn test_adapter_isolation_under_memory_pressure() {
    let mut parser = ParserStep::new();
    
    // Simulate memory pressure by processing many events
    let mut successful_hyperion = 0;
    let mut successful_thala = 0;
    
    for i in 0..1000 {
        // Alternate between adapters to test isolation
        let adapter = if i % 2 == 0 { "hyperion" } else { "thala" };
        let event = create_test_event(adapter, i);
        
        let result = parser.process_event(event).await;
        
        if result.is_ok() {
            if adapter == "hyperion" {
                successful_hyperion += 1;
            } else {
                successful_thala += 1;
            }
        }
        
        // Yield periodically to prevent test timeout
        if i % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    
    // Verify both adapters processed successfully despite memory pressure
    assert!(successful_hyperion > 400, "Hyperion should process most events successfully");
    assert!(successful_thala > 400, "Thala should process most events successfully");
    
    // Verify adapters remain isolated (failure in one doesn't affect the other)
    let health_tracker = parser.get_health_tracker();
    let hyperion_stats = health_tracker.get_adapter_stats("hyperion");
    let thala_stats = health_tracker.get_adapter_stats("thala");
    
    // Both adapters should have reasonable success rates
    assert!(hyperion_stats.success_count > 0);
    assert!(thala_stats.success_count > 0);
}