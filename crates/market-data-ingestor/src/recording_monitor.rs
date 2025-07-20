use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{info, warn};
use serde::{Deserialize, Serialize};

use crate::recording_config::MonitoringSettings;

/// Statistics for recording operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecordingStats {
    // Recording metrics
    pub batches_recorded: u64,
    pub transactions_recorded: u64,
    pub bytes_written: u64,
    pub files_created: u32,
    
    // Pool discovery metrics
    pub pools_discovered: u64,
    pub pools_accepted: u64,
    pub pools_rejected: u64,
    pub pool_states_fetched: u64,
    pub pool_fetch_failures: u64,
    
    // Performance metrics
    pub recording_start_time: Option<SystemTime>,
    pub last_batch_time: Option<SystemTime>,
    pub avg_batch_processing_ms: f64,
    pub max_batch_processing_ms: u64,
    pub min_batch_processing_ms: u64,
    
    // Error metrics
    pub connection_errors: u64,
    pub parsing_errors: u64,
    pub write_errors: u64,
    pub pool_fetch_timeouts: u64,
    
    // Rate metrics (calculated)
    pub batches_per_second: f64,
    pub transactions_per_second: f64,
    pub bytes_per_second: f64,
}

impl RecordingStats {
    pub fn new() -> Self {
        Self {
            recording_start_time: Some(SystemTime::now()),
            min_batch_processing_ms: u64::MAX,
            ..Default::default()
        }
    }
    
    /// Update batch processing metrics
    pub fn record_batch_processed(&mut self, processing_time_ms: u64, transaction_count: u64, bytes_written: u64) {
        self.batches_recorded += 1;
        self.transactions_recorded += transaction_count;
        self.bytes_written += bytes_written;
        self.last_batch_time = Some(SystemTime::now());
        
        // Update processing time metrics
        if processing_time_ms > self.max_batch_processing_ms {
            self.max_batch_processing_ms = processing_time_ms;
        }
        if processing_time_ms < self.min_batch_processing_ms {
            self.min_batch_processing_ms = processing_time_ms;
        }
        
        // Update average (simple moving average)
        let total_batches = self.batches_recorded as f64;
        self.avg_batch_processing_ms = 
            (self.avg_batch_processing_ms * (total_batches - 1.0) + processing_time_ms as f64) / total_batches;
        
        // Update rate metrics
        self.update_rates();
    }
    
    /// Update pool discovery metrics
    pub fn record_pool_discovered(&mut self, accepted: bool) {
        self.pools_discovered += 1;
        if accepted {
            self.pools_accepted += 1;
        } else {
            self.pools_rejected += 1;
        }
    }
    
    /// Record successful pool state fetch
    pub fn record_pool_state_fetched(&mut self) {
        self.pool_states_fetched += 1;
    }
    
    /// Record pool fetch failure
    pub fn record_pool_fetch_failure(&mut self, is_timeout: bool) {
        self.pool_fetch_failures += 1;
        if is_timeout {
            self.pool_fetch_timeouts += 1;
        }
    }
    
    /// Record various error types
    pub fn record_connection_error(&mut self) {
        self.connection_errors += 1;
    }
    
    pub fn record_parsing_error(&mut self) {
        self.parsing_errors += 1;
    }
    
    pub fn record_write_error(&mut self) {
        self.write_errors += 1;
    }
    
    pub fn record_file_created(&mut self) {
        self.files_created += 1;
    }
    
    /// Update rate calculations
    fn update_rates(&mut self) {
        if let Some(start_time) = self.recording_start_time {
            if let Ok(duration) = SystemTime::now().duration_since(start_time) {
                let seconds = duration.as_secs_f64();
                if seconds > 0.0 {
                    self.batches_per_second = self.batches_recorded as f64 / seconds;
                    self.transactions_per_second = self.transactions_recorded as f64 / seconds;
                    self.bytes_per_second = self.bytes_written as f64 / seconds;
                }
            }
        }
    }
    
    /// Get recording duration
    pub fn recording_duration(&self) -> Option<Duration> {
        self.recording_start_time
            .and_then(|start| SystemTime::now().duration_since(start).ok())
    }
    
    /// Format stats for display
    pub fn format_summary(&self) -> String {
        let duration = self.recording_duration()
            .map(|d| format!("{:.1}s", d.as_secs_f64()))
            .unwrap_or_else(|| "unknown".to_string());
        
        format!(
            "Recording Summary:\n\
             Duration: {}\n\
             Batches: {} ({:.2}/s)\n\
             Transactions: {} ({:.2}/s)\n\
             Data Written: {:.2} MB ({:.2} MB/s)\n\
             Files Created: {}\n\
             Pools Discovered: {} (accepted: {}, rejected: {})\n\
             Pool States Fetched: {} (failures: {})\n\
             Avg Batch Time: {:.2}ms (min: {}ms, max: {}ms)\n\
             Errors: conn={}, parse={}, write={}, timeouts={}",
            duration,
            self.batches_recorded, self.batches_per_second,
            self.transactions_recorded, self.transactions_per_second,
            self.bytes_written as f64 / 1024.0 / 1024.0, self.bytes_per_second / 1024.0 / 1024.0,
            self.files_created,
            self.pools_discovered, self.pools_accepted, self.pools_rejected,
            self.pool_states_fetched, self.pool_fetch_failures,
            self.avg_batch_processing_ms, 
            if self.min_batch_processing_ms == u64::MAX { 0 } else { self.min_batch_processing_ms },
            self.max_batch_processing_ms,
            self.connection_errors, self.parsing_errors, self.write_errors, self.pool_fetch_timeouts
        )
    }
    
    /// Format brief progress update
    pub fn format_progress(&self) -> String {
        format!(
            "Progress: {} batches, {} txns, {:.1} MB, {:.2} batches/s",
            self.batches_recorded,
            self.transactions_recorded,
            self.bytes_written as f64 / 1024.0 / 1024.0,
            self.batches_per_second
        )
    }
}

/// Recording monitor that handles periodic reporting and health checks
pub struct RecordingMonitor {
    stats: Arc<RwLock<RecordingStats>>,
    settings: MonitoringSettings,
    start_time: Instant,
}

impl RecordingMonitor {
    pub fn new(settings: MonitoringSettings) -> Self {
        Self {
            stats: Arc::new(RwLock::new(RecordingStats::new())),
            settings,
            start_time: Instant::now(),
        }
    }
    
    /// Get a handle to the stats for updating
    pub fn stats_handle(&self) -> Arc<RwLock<RecordingStats>> {
        self.stats.clone()
    }
    
    /// Start monitoring tasks
    pub async fn start_monitoring(&self) {
        let stats_handle = self.stats.clone();
        let settings = self.settings.clone();
        
        // Stats reporting task
        if settings.stats_interval_seconds > 0 {
            let stats_clone = stats_handle.clone();
            let interval_secs = settings.stats_interval_seconds;
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(interval_secs as u64));
                loop {
                    interval.tick().await;
                    let stats = stats_clone.read().await;
                    info!("{}", stats.format_progress());
                }
            });
        }
        
        // Health check task
        if settings.health_check_interval_seconds > 0 {
            let stats_clone = stats_handle.clone();
            let interval_secs = settings.health_check_interval_seconds;
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_secs(interval_secs as u64));
                let mut last_batch_count = 0u64;
                
                loop {
                    interval.tick().await;
                    let stats = stats_clone.read().await;
                    
                    // Check if we're making progress
                    if stats.batches_recorded == last_batch_count && stats.batches_recorded > 0 {
                        warn!("Health check: No new batches processed in the last {} seconds", interval_secs);
                    }
                    
                    // Check error rates
                    let total_operations = stats.batches_recorded + stats.pools_discovered;
                    let total_errors = stats.connection_errors + stats.parsing_errors + 
                                     stats.write_errors + stats.pool_fetch_failures;
                    
                    if total_operations > 0 {
                        let error_rate = total_errors as f64 / total_operations as f64;
                        if error_rate > 0.1 { // 10% error rate threshold
                            warn!("Health check: High error rate detected: {:.2}% ({}/{})", 
                                  error_rate * 100.0, total_errors, total_operations);
                        }
                    }
                    
                    last_batch_count = stats.batches_recorded;
                }
            });
        }
    }
    
    /// Print final summary
    pub async fn print_final_summary(&self) {
        let stats = self.stats.read().await;
        info!("\n{}", stats.format_summary());
    }
    
    /// Get current stats snapshot
    pub async fn get_stats(&self) -> RecordingStats {
        self.stats.read().await.clone()
    }
    
    /// Check if recording should continue based on limits
    pub async fn should_continue(&self, max_batches: u64, max_duration_seconds: u64) -> bool {
        let stats = self.stats.read().await;
        
        // Check batch limit
        if max_batches > 0 && stats.batches_recorded >= max_batches {
            info!("Reached maximum batch limit: {}", max_batches);
            return false;
        }
        
        // Check duration limit
        if max_duration_seconds > 0 {
            let elapsed = self.start_time.elapsed().as_secs();
            if elapsed >= max_duration_seconds {
                info!("Reached maximum duration limit: {}s", max_duration_seconds);
                return false;
            }
        }
        
        true
    }
}

/// Progress reporter for batch-based progress updates
pub struct ProgressReporter {
    stats: Arc<RwLock<RecordingStats>>,
    report_interval: u32,
    last_reported_batch: u64,
}

impl ProgressReporter {
    pub fn new(stats: Arc<RwLock<RecordingStats>>, report_interval: u32) -> Self {
        Self {
            stats,
            report_interval,
            last_reported_batch: 0,
        }
    }
    
    /// Check if progress should be reported and do so if needed
    pub async fn maybe_report_progress(&mut self) {
        let stats = self.stats.read().await;
        
        if self.report_interval > 0 && 
           stats.batches_recorded >= self.last_reported_batch + self.report_interval as u64 {
            info!("{}", stats.format_progress());
            self.last_reported_batch = stats.batches_recorded;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_recording_stats_creation() {
        let stats = RecordingStats::new();
        assert_eq!(stats.batches_recorded, 0);
        assert!(stats.recording_start_time.is_some());
        assert_eq!(stats.min_batch_processing_ms, u64::MAX);
    }
    
    #[test]
    fn test_batch_processing_metrics() {
        let mut stats = RecordingStats::new();
        
        stats.record_batch_processed(100, 50, 1024);
        assert_eq!(stats.batches_recorded, 1);
        assert_eq!(stats.transactions_recorded, 50);
        assert_eq!(stats.bytes_written, 1024);
        assert_eq!(stats.avg_batch_processing_ms, 100.0);
        assert_eq!(stats.max_batch_processing_ms, 100);
        assert_eq!(stats.min_batch_processing_ms, 100);
        
        stats.record_batch_processed(200, 30, 512);
        assert_eq!(stats.batches_recorded, 2);
        assert_eq!(stats.transactions_recorded, 80);
        assert_eq!(stats.bytes_written, 1536);
        assert_eq!(stats.avg_batch_processing_ms, 150.0);
        assert_eq!(stats.max_batch_processing_ms, 200);
        assert_eq!(stats.min_batch_processing_ms, 100);
    }
    
    #[test]
    fn test_pool_discovery_metrics() {
        let mut stats = RecordingStats::new();
        
        stats.record_pool_discovered(true);
        stats.record_pool_discovered(false);
        stats.record_pool_discovered(true);
        
        assert_eq!(stats.pools_discovered, 3);
        assert_eq!(stats.pools_accepted, 2);
        assert_eq!(stats.pools_rejected, 1);
    }
    
    #[test]
    fn test_error_metrics() {
        let mut stats = RecordingStats::new();
        
        stats.record_connection_error();
        stats.record_parsing_error();
        stats.record_write_error();
        stats.record_pool_fetch_failure(true);
        stats.record_pool_fetch_failure(false);
        
        assert_eq!(stats.connection_errors, 1);
        assert_eq!(stats.parsing_errors, 1);
        assert_eq!(stats.write_errors, 1);
        assert_eq!(stats.pool_fetch_failures, 2);
        assert_eq!(stats.pool_fetch_timeouts, 1);
    }
    
    #[tokio::test]
    async fn test_recording_monitor() {
        let settings = MonitoringSettings {
            stats_interval_seconds: 0, // Disable for test
            progress_report_interval: 100,
            health_check_interval_seconds: 0, // Disable for test
            verbose_logging: false,
            log_file: None,
        };
        
        let monitor = RecordingMonitor::new(settings);
        let stats_handle = monitor.stats_handle();
        
        // Update some stats
        {
            let mut stats = stats_handle.write().await;
            stats.record_batch_processed(100, 10, 1024);
        }
        
        let stats = monitor.get_stats().await;
        assert_eq!(stats.batches_recorded, 1);
        assert_eq!(stats.transactions_recorded, 10);
    }
}