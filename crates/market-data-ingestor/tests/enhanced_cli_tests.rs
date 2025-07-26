use anyhow::Result;
use std::fs;
use tempfile::TempDir;

use market_data_ingestor::file_rotation::FileRotationManager;
use market_data_ingestor::recording_config::RecordingConfig;
use market_data_ingestor::recording_monitor::{RecordingMonitor, RecordingStats};

#[test]
fn test_recording_config_default() {
    let config = RecordingConfig::default();

    assert!(config.pool_detection.enabled);
    assert!(config.recording.file_rotation.enabled);
    assert_eq!(config.recording.file_rotation.max_size_mb, 1000);
    assert_eq!(config.recording.file_rotation.max_files, 10);
    assert_eq!(config.pool_detection.filters.max_tracked_pools, 5000);
    assert_eq!(config.pool_detection.workers.pool_size, 20);
    assert_eq!(config.monitoring.stats_interval_seconds, 10);
}

#[test]
fn test_recording_config_validation() {
    let mut config = RecordingConfig::default();

    // Valid config should pass
    assert!(config.validate().is_ok());

    // Invalid max_size_mb should fail
    config.recording.file_rotation.max_size_mb = 0;
    assert!(config.validate().is_err());

    // Reset and test invalid max_files
    config = RecordingConfig::default();
    config.recording.file_rotation.max_files = 0;
    assert!(config.validate().is_err());

    // Reset and test invalid pool_size
    config = RecordingConfig::default();
    config.pool_detection.workers.pool_size = 0;
    assert!(config.validate().is_err());
}

#[test]
fn test_recording_config_yaml_serialization() -> Result<()> {
    let config = RecordingConfig::default();

    // Serialize to YAML
    let yaml_content = serde_yaml::to_string(&config)?;

    // Deserialize back
    let deserialized: RecordingConfig = serde_yaml::from_str(&yaml_content)?;

    // Should be equivalent
    assert_eq!(
        config.recording.file_rotation.enabled,
        deserialized.recording.file_rotation.enabled
    );
    assert_eq!(
        config.pool_detection.enabled,
        deserialized.pool_detection.enabled
    );
    assert_eq!(
        config.monitoring.stats_interval_seconds,
        deserialized.monitoring.stats_interval_seconds
    );

    Ok(())
}

#[test]
fn test_file_rotation_manager_creation() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let output_path = temp_dir.path().join("test_{timestamp}.pb");

    let settings = market_data_ingestor::recording_config::FileRotationSettings {
        enabled: true,
        max_size_mb: 1,
        max_files: 5,
        compress_rotated: false,
    };

    let manager = FileRotationManager::new(output_path.to_str().unwrap(), settings);

    assert!(manager.is_ok());
    Ok(())
}

#[test]
fn test_file_rotation_manager_writing() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let output_path = temp_dir.path().join("test.pb");

    let settings = market_data_ingestor::recording_config::FileRotationSettings {
        enabled: false, // Disable rotation for simple test
        max_size_mb: 1,
        max_files: 5,
        compress_rotated: false,
    };

    let mut manager = FileRotationManager::new(output_path.to_str().unwrap(), settings)?;

    let test_data = b"test data for file rotation";
    manager.write(test_data)?;
    manager.flush()?;

    assert_eq!(manager.current_file_size(), test_data.len() as u64);
    assert!(output_path.exists());

    let written_data = fs::read(&output_path)?;
    assert_eq!(written_data, test_data);

    Ok(())
}

#[tokio::test]
async fn test_recording_monitor_creation() {
    let settings = market_data_ingestor::recording_config::MonitoringSettings {
        stats_interval_seconds: 0, // Disable for test
        progress_report_interval: 100,
        health_check_interval_seconds: 0, // Disable for test
        verbose_logging: false,
        log_file: None,
    };

    let monitor = RecordingMonitor::new(settings);
    let stats_handle = monitor.stats_handle();

    // Test initial stats
    let stats = stats_handle.read().await;
    assert_eq!(stats.batches_recorded, 0);
    assert_eq!(stats.transactions_recorded, 0);
    assert!(stats.recording_start_time.is_some());
}

#[tokio::test]
async fn test_recording_stats_updates() {
    let settings = market_data_ingestor::recording_config::MonitoringSettings {
        stats_interval_seconds: 0,
        progress_report_interval: 100,
        health_check_interval_seconds: 0,
        verbose_logging: false,
        log_file: None,
    };

    let monitor = RecordingMonitor::new(settings);
    let stats_handle = monitor.stats_handle();

    // Update stats
    {
        let mut stats = stats_handle.write().await;
        stats.record_batch_processed(100, 10, 1024);
        stats.record_pool_discovered(true);
        stats.record_pool_state_fetched();
    }

    // Verify updates
    let stats = stats_handle.read().await;
    assert_eq!(stats.batches_recorded, 1);
    assert_eq!(stats.transactions_recorded, 10);
    assert_eq!(stats.bytes_written, 1024);
    assert_eq!(stats.pools_discovered, 1);
    assert_eq!(stats.pools_accepted, 1);
    assert_eq!(stats.pool_states_fetched, 1);
    assert_eq!(stats.avg_batch_processing_ms, 100.0);
}

#[test]
fn test_recording_stats_formatting() {
    let mut stats = RecordingStats::new();

    // Add some test data
    stats.record_batch_processed(150, 25, 2048);
    stats.record_batch_processed(200, 30, 1536);
    stats.record_pool_discovered(true);
    stats.record_pool_discovered(false);
    stats.record_pool_state_fetched();
    stats.record_connection_error();

    let summary = stats.format_summary();
    assert!(summary.contains("Batches: 2"));
    assert!(summary.contains("Transactions: 55"));
    assert!(summary.contains("Pools Discovered: 2"));
    assert!(summary.contains("accepted: 1, rejected: 1"));
    assert!(summary.contains("Errors: conn=1"));

    let progress = stats.format_progress();
    assert!(progress.contains("2 batches"));
    assert!(progress.contains("55 txns"));
}

#[tokio::test]
async fn test_monitor_should_continue() {
    let settings = market_data_ingestor::recording_config::MonitoringSettings {
        stats_interval_seconds: 0,
        progress_report_interval: 100,
        health_check_interval_seconds: 0,
        verbose_logging: false,
        log_file: None,
    };

    let monitor = RecordingMonitor::new(settings);

    // Should continue with no limits
    assert!(monitor.should_continue(0, 0).await);

    // Should continue under batch limit
    assert!(monitor.should_continue(100, 0).await);

    // Should stop when batch limit reached
    {
        let stats_handle = monitor.stats_handle();
        let mut stats = stats_handle.write().await;
        stats.record_batch_processed(100, 10, 1024);
        stats.record_batch_processed(100, 10, 1024);
    }

    assert!(!monitor.should_continue(1, 0).await);
}

#[test]
fn test_timestamp_substitution() -> Result<()> {
    let pattern = "recording_{timestamp}.pb";
    let resolved =
        market_data_ingestor::file_rotation::FileRotationManager::resolve_output_path(pattern)?;
    let path_str = resolved.to_string_lossy();

    assert!(path_str.starts_with("recording_"));
    assert!(path_str.ends_with(".pb"));
    assert!(!path_str.contains("{timestamp}"));

    Ok(())
}

#[test]
fn test_file_rotation_size_check() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let output_path = temp_dir.path().join("test.pb");

    let settings = market_data_ingestor::recording_config::FileRotationSettings {
        enabled: true,
        max_size_mb: 1, // 1MB limit
        max_files: 5,
        compress_rotated: false,
    };

    let mut manager = FileRotationManager::new(output_path.to_str().unwrap(), settings)?;

    // Write data that should trigger rotation
    let large_data = vec![0u8; 1024 * 1024 + 1]; // Just over 1MB
    manager.write(&large_data)?;

    // Should have rotated to a new file
    assert_eq!(manager.file_counter(), 1);

    Ok(())
}

#[test]
fn test_pool_filter_config_creation() {
    let recording_config = RecordingConfig::default();

    // Test creating pool filter config from recording config
    let pool_filter = market_data_ingestor::pool_state_manager::PoolFilterConfig {
        min_tvl_usd: recording_config.pool_detection.filters.min_tvl_usd,
        token_whitelist: None,
        token_blacklist: recording_config
            .pool_detection
            .filters
            .token_blacklist
            .clone(),
        dex_whitelist: recording_config
            .pool_detection
            .filters
            .dex_whitelist
            .clone(),
        pool_type_whitelist: recording_config
            .pool_detection
            .filters
            .pool_type_whitelist
            .clone(),
        max_tracked_pools: Some(recording_config.pool_detection.filters.max_tracked_pools as usize),
        max_cache_size_per_block: Some(500),
        max_cache_retention_seconds: Some(30),
    };

    assert_eq!(pool_filter.max_tracked_pools, Some(5000));
    assert!(pool_filter.min_tvl_usd.is_none());
    assert!(pool_filter.token_whitelist.is_none());
}

#[test]
fn test_config_cli_overrides() {
    let mut config = RecordingConfig::default();

    // Simulate CLI overrides
    config.recording.max_batches = 1000;
    config.recording.max_duration_seconds = 3600;
    config.recording.file_rotation.enabled = false;

    assert_eq!(config.recording.max_batches, 1000);
    assert_eq!(config.recording.max_duration_seconds, 3600);
    assert!(!config.recording.file_rotation.enabled);
}

#[test]
fn test_worker_settings_validation() {
    let config = RecordingConfig::default();
    let workers = &config.pool_detection.workers;

    assert!(workers.pool_size > 0);
    assert!(workers.timeout_seconds > 0);
    assert!(workers.retry_attempts > 0);
    assert!(workers.rate_limit_per_minute > 0);
}

#[test]
fn test_monitoring_settings_validation() {
    let config = RecordingConfig::default();
    let monitoring = &config.monitoring;

    assert!(monitoring.stats_interval_seconds > 0);
    assert!(monitoring.progress_report_interval > 0);
    assert!(monitoring.health_check_interval_seconds > 0);
    assert!(!monitoring.verbose_logging); // Default should be false
    assert!(monitoring.log_file.is_none()); // Default should be None
}
