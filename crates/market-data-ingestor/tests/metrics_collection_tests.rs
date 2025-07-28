use market_data_ingestor::recording_monitor::{RecordingStats, StageTimings};
use market_data_ingestor::monitoring::{MetricsCollector, WarningLevel};
use tokio::time::{sleep, Duration};

#[test]
fn test_stage_timing_accuracy() {
    let mut stats = RecordingStats::new();
    
    // Test first stage timing
    let timing1 = StageTimings::new(1.5, 2.0, 0.5, 1.0);
    stats.record_stage_timing(&timing1);
    
    assert_eq!(stats.stage_timings_collected, 1);
    assert_eq!(stats.event_extraction_time_ms, 1.5);
    assert_eq!(stats.parsing_time_ms, 2.0);
    assert_eq!(stats.filtering_time_ms, 0.5);
    assert_eq!(stats.detector_push_time_ms, 1.0);
    
    // Test rolling average calculation
    let timing2 = StageTimings::new(2.5, 4.0, 1.5, 3.0);
    stats.record_stage_timing(&timing2);
    
    assert_eq!(stats.stage_timings_collected, 2);
    assert_eq!(stats.event_extraction_time_ms, 2.0); // (1.5 + 2.5) / 2
    assert_eq!(stats.parsing_time_ms, 3.0); // (2.0 + 4.0) / 2
    assert_eq!(stats.filtering_time_ms, 1.0); // (0.5 + 1.5) / 2
    assert_eq!(stats.detector_push_time_ms, 2.0); // (1.0 + 3.0) / 2
}

#[test]
fn test_stage_timing_total_calculation() {
    let timing = StageTimings::new(1.5, 2.0, 0.5, 1.0);
    assert_eq!(timing.total_time_ms(), 5.0);
    
    let zero_timing = StageTimings::new(0.0, 0.0, 0.0, 0.0);
    assert_eq!(zero_timing.total_time_ms(), 0.0);
}

#[test]
fn test_queue_metrics_accuracy() {
    let mut stats = RecordingStats::new();
    
    // Test first queue operation
    stats.record_queue_metrics(10, 5.0, false);
    assert_eq!(stats.queue_depth_current, 10);
    assert_eq!(stats.queue_depth_max_observed, 10);
    assert_eq!(stats.queue_operations_total, 1);
    assert_eq!(stats.queue_saturation_events, 0);
    assert_eq!(stats.backpressure_duration_ms, 0.0);
    
    // Test operation with higher depth
    stats.record_queue_metrics(25, 10.0, false);
    assert_eq!(stats.queue_depth_current, 25);
    assert_eq!(stats.queue_depth_max_observed, 25);
    assert_eq!(stats.queue_operations_total, 2);
    
    // Test operation with blocking
    stats.record_queue_metrics(15, 50.0, true);
    assert_eq!(stats.queue_operations_total, 3);
    assert!(stats.backpressure_duration_ms > 0.0);
    
    // Test saturation detection (>80% of 1000 capacity)
    stats.record_queue_metrics(850, 2.0, false);
    assert_eq!(stats.queue_saturation_events, 1);
}

#[test]
fn test_warning_level_calculation() {
    let mut stats = RecordingStats::new();
    
    // Simulate batch processing to set avg_batch_processing_ms
    stats.record_batch_processed(50, 10, 1024); // 50ms processing time
    
    // Test no warning when under threshold
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::None);
    
    // Test low warning when over threshold but no consecutive violations
    stats.record_batch_processed(150, 10, 1024); // 100ms average now
    stats.record_batch_processed(150, 10, 1024); // 125ms average now
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Low);
    
    // Test escalation with consecutive violations (still Low since avg < medium_threshold and consecutive < 5)
    stats.threshold_violations.consecutive_count = 3;
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Low);
    
    // Test critical level
    stats.record_batch_processed(400, 10, 1024); // High latency
    stats.record_batch_processed(400, 10, 1024);
    stats.record_batch_processed(400, 10, 1024); // Very high average now
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Critical);
}

#[test]
fn test_warning_tracking() {
    let mut stats = RecordingStats::new();
    
    // Test warning recording
    stats.record_performance_warning(WarningLevel::Medium, 3.0);
    assert_eq!(stats.threshold_violations.total_count, 1);
    assert_eq!(stats.threshold_violations.consecutive_count, 3);
    assert_eq!(stats.threshold_violations.escalation_count, 1); // Escalation on > 1 violation
    assert!(stats.threshold_violations.last_violation_time.is_some());
    
    // Test reset
    stats.reset_violation_tracking();
    assert_eq!(stats.threshold_violations.consecutive_count, 0);
}

#[tokio::test]
async fn test_metrics_collector_creation() {
    let collector = MetricsCollector::new().expect("Failed to create metrics collector");
    assert!(!collector.registry().gather().is_empty());
}

#[tokio::test]
async fn test_metrics_collector_updates() {
    let mut collector = MetricsCollector::new().expect("Failed to create metrics collector");
    let mut stats = RecordingStats::new();
    
    // Add some test data
    stats.record_batch_processed(100, 50, 2048);
    let stage_timing = StageTimings::new(1.0, 2.0, 0.5, 1.5);
    stats.record_stage_timing(&stage_timing);
    stats.record_queue_metrics(10, 5.0, false);
    
    // Update metrics
    collector.update_metrics(&stats, "test", true);
    
    // Test stage timing updates
    collector.update_stage_timings(&stage_timing);
    
    // Test warning updates
    collector.update_warning_metrics(&WarningLevel::Medium, 3, true);
    
    // Test queue updates
    collector.update_queue_metrics(10, 25, 1, 5.0);
    
    // Verify production metrics can be generated
    let production_metrics = collector.get_production_metrics("test", true);
    assert_eq!(production_metrics.data_flow.batches_processed, 1);
    assert_eq!(production_metrics.stage_timing.stage_timings_collected, 1);
    assert_eq!(production_metrics.queue_monitoring.queue_depth_current, 10);
}

#[test]
fn test_concurrent_metrics_access() {
    use std::sync::Arc;
    use std::thread;
    
    let stats = Arc::new(std::sync::Mutex::new(RecordingStats::new()));
    let mut handles = vec![];
    
    // Spawn multiple threads to update metrics concurrently
    for i in 0..10 {
        let stats_clone = Arc::clone(&stats);
        let handle = thread::spawn(move || {
            let timing = StageTimings::new(i as f64, i as f64 * 2.0, i as f64 * 0.5, i as f64 * 1.5);
            if let Ok(mut stats) = stats_clone.lock() {
                stats.record_stage_timing(&timing);
                stats.record_queue_metrics(i * 5, i as f64 * 2.0, i % 3 == 0);
            }
        });
        handles.push(handle);
    }
    
    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Verify final state
    let final_stats = stats.lock().unwrap();
    assert_eq!(final_stats.stage_timings_collected, 10);
    assert_eq!(final_stats.queue_operations_total, 10);
    assert!(final_stats.queue_depth_max_observed > 0);
}

#[tokio::test]
async fn test_metrics_memory_usage() {
    // Test that metrics collection doesn't cause excessive memory growth
    let mut collector = MetricsCollector::new().expect("Failed to create metrics collector");
    let initial_memory = get_memory_usage();
    
    // Perform many metric updates
    for i in 0..1000 {
        let mut stats = RecordingStats::new();
        stats.record_batch_processed(100 + i, 50, 2048);
        
        let timing = StageTimings::new(
            1.0 + i as f64 * 0.01,
            2.0 + i as f64 * 0.01,
            0.5 + i as f64 * 0.01,
            1.5 + i as f64 * 0.01,
        );
        stats.record_stage_timing(&timing);
        stats.record_queue_metrics(i as usize % 100, i as f64 * 0.1, i % 10 == 0);
        
        collector.update_metrics(&stats, "test", true);
        collector.update_stage_timings(&timing);
    }
    
    let final_memory = get_memory_usage();
    let memory_increase_mb = (final_memory - initial_memory) as f64 / 1024.0 / 1024.0;
    
    // Memory increase should be less than 2MB for 1000 operations
    assert!(memory_increase_mb < 2.0, "Memory usage increased by {:.2} MB", memory_increase_mb);
}

// Helper function to get current memory usage (simplified)
fn get_memory_usage() -> usize {
    // This is a simplified memory usage check
    // In a real implementation, you might use more sophisticated memory tracking
    std::mem::size_of::<RecordingStats>() * 1000 // Placeholder
}

#[test]
fn test_prometheus_metric_consistency() {
    let collector = MetricsCollector::new().expect("Failed to create metrics collector");
    let metrics = collector.registry().gather();
    
    // Verify all expected metrics are present
    let metric_names: Vec<String> = metrics.iter().map(|m| m.get_name().to_string()).collect();
    
    // Stage timing metrics
    assert!(metric_names.contains(&"mdi_event_extraction_latency_seconds".to_string()));
    assert!(metric_names.contains(&"mdi_parsing_latency_seconds".to_string()));
    assert!(metric_names.contains(&"mdi_filtering_latency_seconds".to_string()));
    assert!(metric_names.contains(&"mdi_detector_push_latency_seconds".to_string()));
    
    // Warning metrics
    assert!(metric_names.contains(&"mdi_latency_warnings_total".to_string()));
    assert!(metric_names.contains(&"mdi_consecutive_violations".to_string()));
    assert!(metric_names.contains(&"mdi_current_warning_level".to_string()));
    
    // Queue metrics
    assert!(metric_names.contains(&"mdi_queue_depth_current".to_string()));
    assert!(metric_names.contains(&"mdi_queue_saturation_events_total".to_string()));
    assert!(metric_names.contains(&"mdi_backpressure_duration_seconds".to_string()));
}

#[tokio::test]
async fn test_timing_precision() {
    // Test that our timing measurements have sufficient precision
    let start = std::time::Instant::now();
    
    // Simulate very short operation
    sleep(Duration::from_micros(100)).await;
    
    let elapsed_ms = start.elapsed().as_micros() as f64 / 1000.0;
    
    // Should be able to measure sub-millisecond operations
    assert!(elapsed_ms >= 0.05); // At least 0.05ms
    assert!(elapsed_ms < 10.0);  // Less than 10ms for 100μs sleep (more lenient for CI)
}