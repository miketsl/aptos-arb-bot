use market_data_ingestor::monitoring::{WarningLevel, MetricsCollector};
use market_data_ingestor::recording_monitor::RecordingStats;
use std::time::{Duration, Instant, SystemTime};
use tokio::time::sleep;

#[test]
fn test_warning_level_transitions() {
    let mut stats = RecordingStats::new();
    
    // Start with no warnings
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::None);
    
    // Simulate latency above threshold
    stats.record_batch_processed(120, 10, 1024);
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Low); // First violation
    
    // Add consecutive violations
    stats.threshold_violations.consecutive_count = 5;
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Low); // Still low with few violations
    
    stats.threshold_violations.consecutive_count = 3;
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Medium); // Escalated to medium
    
    // Increase latency further
    stats.record_batch_processed(180, 10, 1024); // Higher latency
    stats.threshold_violations.consecutive_count = 5;
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::High); // High warning
    
    // Critical threshold (2.0 * 100.0 = 200.0)
    stats.record_batch_processed(250, 10, 1024);
    stats.record_batch_processed(250, 10, 1024); // Push average over critical
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Critical); // Critical warning
}

#[test]
fn test_warning_escalation_logic() {
    let mut stats = RecordingStats::new();
    
    // Test escalation counting
    assert_eq!(stats.threshold_violations.escalation_count, 0);
    
    // First warning (no escalation)
    stats.record_performance_warning(WarningLevel::Low, 1.0);
    assert_eq!(stats.threshold_violations.escalation_count, 0);
    
    // Second consecutive warning (escalation)
    stats.record_performance_warning(WarningLevel::Medium, 2.0);
    assert_eq!(stats.threshold_violations.escalation_count, 1);
    
    // Third consecutive warning (another escalation)
    stats.record_performance_warning(WarningLevel::High, 3.0);
    assert_eq!(stats.threshold_violations.escalation_count, 2);
    
    // Verify warning count
    assert_eq!(stats.threshold_violations.total_count, 3);
    assert_eq!(stats.threshold_violations.consecutive_count, 3);
    assert!(stats.threshold_violations.last_violation_time.is_some());
}

#[test]
fn test_threshold_configuration_edge_cases() {
    let mut stats = RecordingStats::new();
    
    // Test with very low threshold
    stats.record_batch_processed(1, 10, 1024); // 1ms processing
    let level = stats.calculate_warning_level(0.5, 2.0); // 0.5ms threshold
    assert_eq!(level, WarningLevel::Low);
    
    // Test with very high threshold
    stats.record_batch_processed(50, 10, 1024); // 25.5ms average
    let level = stats.calculate_warning_level(1000.0, 2.0); // 1000ms threshold
    assert_eq!(level, WarningLevel::None);
    
    // Test with extreme multiplier
    stats.record_batch_processed(100, 10, 1024); // Higher average
    let level = stats.calculate_warning_level(10.0, 100.0); // Very high multiplier
    assert_eq!(level, WarningLevel::Low); // Over threshold but not critical
    
    // Test zero threshold (should not panic)
    let level = stats.calculate_warning_level(0.0, 2.0);
    assert_ne!(level, WarningLevel::None); // Everything is over 0 threshold
}

#[tokio::test]
async fn test_warning_rate_limiting() {
    let mut collector = MetricsCollector::new().expect("Failed to create metrics collector");
    let mut last_warning_time = None::<Instant>;
    let rate_limit = Duration::from_millis(100);
    
    // Simulate rapid warning triggers
    for i in 0..10 {
        let now = Instant::now();
        let should_log = last_warning_time.map_or(true, |last| now.duration_since(last) >= rate_limit);
        
        if should_log {
            last_warning_time = Some(now);
            collector.update_warning_metrics(&WarningLevel::High, i, false);
        }
        
        sleep(Duration::from_millis(20)).await; // Rapid iterations
    }
    
    // Verify that rate limiting worked (not all warnings were processed)
    let metrics = collector.registry().gather();
    let warning_metric = metrics.iter()
        .find(|m| m.get_name() == "mdi_latency_warnings_total")
        .expect("Warning metric not found");
    
    let warning_count = warning_metric.get_metric()[0].get_counter().get_value() as u64;
    assert!(warning_count < 10, "Rate limiting failed: {} warnings logged", warning_count);
    assert!(warning_count > 0, "No warnings logged at all");
}

#[test]
fn test_warning_reset_behavior() {
    let mut stats = RecordingStats::new();
    
    // Build up violations
    stats.threshold_violations.consecutive_count = 5;
    stats.threshold_violations.total_count = 10;
    stats.threshold_violations.escalation_count = 3;
    
    // Reset violations
    stats.reset_violation_tracking();
    
    // Verify only consecutive violations are reset
    assert_eq!(stats.threshold_violations.consecutive_count, 0);
    assert_eq!(stats.threshold_violations.total_count, 10); // Preserved
    assert_eq!(stats.threshold_violations.escalation_count, 3); // Preserved
}

#[tokio::test]
async fn test_warning_metrics_prometheus_integration() {
    let collector = MetricsCollector::new().expect("Failed to create metrics collector");
    
    // Test warning level updates
    collector.update_warning_metrics(&WarningLevel::Critical, 5, true);
    
    // Verify metrics are updated
    let metrics = collector.registry().gather();
    
    // Check warning level metric
    let warning_level_metric = metrics.iter()
        .find(|m| m.get_name() == "mdi_current_warning_level")
        .expect("Warning level metric not found");
    let level_value = warning_level_metric.get_metric()[0].get_gauge().get_value();
    assert_eq!(level_value, 4.0); // Critical = 4
    
    // Check consecutive violations
    let violations_metric = metrics.iter()
        .find(|m| m.get_name() == "mdi_consecutive_violations")
        .expect("Consecutive violations metric not found");
    let violations_value = violations_metric.get_metric()[0].get_gauge().get_value();
    assert_eq!(violations_value, 5.0);
    
    // Test reset
    collector.reset_warning_metrics();
    let metrics_after_reset = collector.registry().gather();
    
    let level_after_reset = metrics_after_reset.iter()
        .find(|m| m.get_name() == "mdi_current_warning_level")
        .unwrap().get_metric()[0].get_gauge().get_value();
    assert_eq!(level_after_reset, 0.0); // None = 0
}

#[test]
fn test_threshold_violation_counting() {
    let mut stats = RecordingStats::new();
    
    // Test that batch processing updates trigger threshold checks
    let threshold_ms = 50.0;
    
    // Process under threshold
    stats.record_batch_processed(30, 10, 1024);
    assert!(stats.avg_batch_processing_ms < threshold_ms);
    
    // Process over threshold
    stats.record_batch_processed(100, 10, 1024); // Should push average over 50
    assert!(stats.avg_batch_processing_ms > threshold_ms);
    
    // Verify warning level calculation
    let level = stats.calculate_warning_level(threshold_ms, 2.0);
    assert_eq!(level, WarningLevel::Low);
}

#[tokio::test] 
async fn test_warning_performance_impact() {
    let start_time = Instant::now();
    let iterations = 1000;
    
    let collector = MetricsCollector::new().expect("Failed to create metrics collector");
    
    // Measure time for warning updates
    let warning_start = Instant::now();
    for i in 0..iterations {
        let level = if i % 4 == 0 { WarningLevel::Critical } 
                   else if i % 3 == 0 { WarningLevel::High }
                   else if i % 2 == 0 { WarningLevel::Medium }
                   else { WarningLevel::Low };
        
        collector.update_warning_metrics(&level, i as u64, i % 10 == 0);
    }
    let warning_duration = warning_start.elapsed();
    
    // Verify performance (should be very fast)
    let avg_time_per_warning = warning_duration.as_micros() as f64 / iterations as f64;
    assert!(avg_time_per_warning < 100.0, 
           "Warning update too slow: {:.2}μs per operation", avg_time_per_warning);
    
    println!("Warning update performance: {:.2}μs per operation", avg_time_per_warning);
}

#[test]
fn test_warning_configuration_validation() {
    use config_lib::PerformanceConfig;
    
    // Test default configuration values
    let config = PerformanceConfig {
        max_events_per_batch: 100,
        batch_timeout_ms: 1000,
        channel_buffer_size: 1000,
        latency_warning_threshold_ms: 100,
        metrics_enabled: true,
        metrics_port: 9090,
        warning_escalation_count: 3,
        critical_latency_multiplier: 2.0,
        warning_log_interval_seconds: 60,
    };
    
    // Verify reasonable defaults
    assert!(config.warning_escalation_count > 0);
    assert!(config.critical_latency_multiplier > 1.0);
    assert!(config.warning_log_interval_seconds > 0);
    assert!(config.latency_warning_threshold_ms > 0);
}

#[tokio::test]
async fn test_concurrent_warning_updates() {
    use std::sync::Arc;
    use tokio::task;
    
    let collector = Arc::new(MetricsCollector::new().expect("Failed to create metrics collector"));
    let mut handles = vec![];
    
    // Spawn concurrent warning updates
    for i in 0..20 {
        let collector_clone = Arc::clone(&collector);
        let handle = task::spawn(async move {
            let level = match i % 5 {
                0 => WarningLevel::None,
                1 => WarningLevel::Low,
                2 => WarningLevel::Medium,
                3 => WarningLevel::High,
                _ => WarningLevel::Critical,
            };
            
            collector_clone.update_warning_metrics(&level, i as u64, i % 5 == 0);
            sleep(Duration::from_millis(1)).await;
        });
        handles.push(handle);
    }
    
    // Wait for all updates to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }
    
    // Verify final state is consistent
    let metrics = collector.registry().gather();
    let warning_total = metrics.iter()
        .find(|m| m.get_name() == "mdi_latency_warnings_total")
        .expect("Warning total metric not found")
        .get_metric()[0].get_counter().get_value();
    
    assert_eq!(warning_total, 20.0, "Expected 20 warnings, got {}", warning_total);
}

#[test]
fn test_edge_case_threshold_scenarios() {
    let mut stats = RecordingStats::new();
    
    // Test exactly at threshold
    stats.record_batch_processed(100, 10, 1024);
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::None); // Exactly at threshold should not warn
    
    // Test just over threshold
    stats.record_batch_processed(101, 10, 1024);
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Low); // Just over threshold should warn
    
    // Test NaN/infinity handling (should not panic)
    stats.avg_batch_processing_ms = f64::NAN;
    let level = stats.calculate_warning_level(100.0, 2.0);
    // Should handle gracefully without panicking
    
    stats.avg_batch_processing_ms = f64::INFINITY;
    let level = stats.calculate_warning_level(100.0, 2.0);
    assert_eq!(level, WarningLevel::Critical); // Infinity should be critical
}