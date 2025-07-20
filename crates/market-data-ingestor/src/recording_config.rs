use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the enhanced mdi-recorder CLI tool
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RecordingConfig {
    pub recording: RecordingSettings,
    pub pool_detection: PoolDetectionSettings,
    pub monitoring: MonitoringSettings,
}

/// Core recording settings
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RecordingSettings {
    /// Output file pattern with timestamp support
    #[serde(default = "default_output_file")]
    pub output_file: String,
    
    /// File rotation settings
    pub file_rotation: FileRotationSettings,
    
    /// Maximum number of batches to record (0 = unlimited)
    #[serde(default)]
    pub max_batches: u64,
    
    /// Maximum recording duration in seconds (0 = unlimited)
    #[serde(default)]
    pub max_duration_seconds: u64,
}

/// File rotation configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FileRotationSettings {
    /// Enable file rotation
    #[serde(default = "default_rotation_enabled")]
    pub enabled: bool,
    
    /// Maximum file size in MB before rotation
    #[serde(default = "default_max_size_mb")]
    pub max_size_mb: u64,
    
    /// Maximum number of files to keep
    #[serde(default = "default_max_files")]
    pub max_files: u32,
    
    /// Compress rotated files
    #[serde(default = "default_compress_rotated")]
    pub compress_rotated: bool,
}

/// Pool detection and filtering settings
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PoolDetectionSettings {
    /// Enable pool state detection and embedding
    #[serde(default = "default_pool_detection_enabled")]
    pub enabled: bool,
    
    /// Pool filtering configuration
    pub filters: PoolFilters,
    
    /// Worker configuration for concurrent pool state fetching
    pub workers: WorkerSettings,
}

/// Pool filtering configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PoolFilters {
    /// Minimum TVL in USD (disabled until price feeds available)
    pub min_tvl_usd: Option<f64>,
    
    /// Maximum number of pools to track simultaneously
    #[serde(default = "default_max_tracked_pools")]
    pub max_tracked_pools: u32,
    
    /// DEX whitelist - only track pools from these DEXes
    pub dex_whitelist: Option<Vec<String>>,
    
    /// Token blacklist - never track pools containing these tokens
    pub token_blacklist: Option<Vec<String>>,
    
    /// Pool type whitelist - only track these pool types
    pub pool_type_whitelist: Option<Vec<String>>,
}

/// Worker pool configuration for concurrent operations
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkerSettings {
    /// Number of concurrent workers for pool state fetching
    #[serde(default = "default_pool_workers")]
    pub pool_size: u32,
    
    /// Timeout for individual pool state fetch operations
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u32,
    
    /// Number of retry attempts for failed operations
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,
    
    /// Rate limit per minute for API calls
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_minute: u32,
}

/// Monitoring and reporting settings
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MonitoringSettings {
    /// Interval for printing statistics (seconds)
    #[serde(default = "default_stats_interval")]
    pub stats_interval_seconds: u32,
    
    /// Progress report interval (number of batches)
    #[serde(default = "default_progress_interval")]
    pub progress_report_interval: u32,
    
    /// Health check interval (seconds)
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_seconds: u32,
    
    /// Enable detailed logging
    #[serde(default = "default_verbose_logging")]
    pub verbose_logging: bool,
    
    /// Log file path (optional)
    pub log_file: Option<PathBuf>,
}

/// Default values
fn default_output_file() -> String {
    "recording_{timestamp}.pb".to_string()
}

fn default_rotation_enabled() -> bool {
    true
}

fn default_max_size_mb() -> u64 {
    1000
}

fn default_max_files() -> u32 {
    10
}

fn default_compress_rotated() -> bool {
    false
}

fn default_pool_detection_enabled() -> bool {
    true
}

fn default_max_tracked_pools() -> u32 {
    5000
}

fn default_pool_workers() -> u32 {
    20
}

fn default_timeout_seconds() -> u32 {
    30
}

fn default_retry_attempts() -> u32 {
    3
}

fn default_rate_limit() -> u32 {
    100
}

fn default_stats_interval() -> u32 {
    10
}

fn default_progress_interval() -> u32 {
    100
}

fn default_health_check_interval() -> u32 {
    60
}

fn default_verbose_logging() -> bool {
    false
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            recording: RecordingSettings {
                output_file: default_output_file(),
                file_rotation: FileRotationSettings::default(),
                max_batches: 0,
                max_duration_seconds: 0,
            },
            pool_detection: PoolDetectionSettings {
                enabled: default_pool_detection_enabled(),
                filters: PoolFilters::default(),
                workers: WorkerSettings::default(),
            },
            monitoring: MonitoringSettings::default(),
        }
    }
}

impl Default for FileRotationSettings {
    fn default() -> Self {
        Self {
            enabled: default_rotation_enabled(),
            max_size_mb: default_max_size_mb(),
            max_files: default_max_files(),
            compress_rotated: default_compress_rotated(),
        }
    }
}

impl Default for PoolFilters {
    fn default() -> Self {
        Self {
            min_tvl_usd: None, // Disabled until price feeds
            max_tracked_pools: default_max_tracked_pools(),
            dex_whitelist: None,
            token_blacklist: None,
            pool_type_whitelist: None,
        }
    }
}

impl Default for WorkerSettings {
    fn default() -> Self {
        Self {
            pool_size: default_pool_workers(),
            timeout_seconds: default_timeout_seconds(),
            retry_attempts: default_retry_attempts(),
            rate_limit_per_minute: default_rate_limit(),
        }
    }
}

impl Default for MonitoringSettings {
    fn default() -> Self {
        Self {
            stats_interval_seconds: default_stats_interval(),
            progress_report_interval: default_progress_interval(),
            health_check_interval_seconds: default_health_check_interval(),
            verbose_logging: default_verbose_logging(),
            log_file: None,
        }
    }
}

/// Validation for recording configuration
impl RecordingConfig {
    pub fn validate(&self) -> Result<(), String> {
        // Validate file rotation settings
        if self.recording.file_rotation.enabled {
            if self.recording.file_rotation.max_size_mb == 0 {
                return Err("max_size_mb must be greater than 0 when file rotation is enabled".to_string());
            }
            if self.recording.file_rotation.max_files == 0 {
                return Err("max_files must be greater than 0 when file rotation is enabled".to_string());
            }
        }
        
        // Validate worker settings
        if self.pool_detection.workers.pool_size == 0 {
            return Err("worker pool_size must be greater than 0".to_string());
        }
        
        if self.pool_detection.workers.timeout_seconds == 0 {
            return Err("worker timeout_seconds must be greater than 0".to_string());
        }
        
        // Validate monitoring settings
        if self.monitoring.stats_interval_seconds == 0 {
            return Err("stats_interval_seconds must be greater than 0".to_string());
        }
        
        if self.monitoring.progress_report_interval == 0 {
            return Err("progress_report_interval must be greater than 0".to_string());
        }
        
        Ok(())
    }
}