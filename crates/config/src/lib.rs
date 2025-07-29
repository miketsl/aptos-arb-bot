use detector::strategies::StrategyConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Enhanced configuration structures for Task 1

/// Configuration for the Market Data Ingestor component
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IngestorConfig {
    pub data_source: DataSourceConfig,
    pub filters: IngestorFilterConfig,
    pub performance: PerformanceConfig,
    pub adapters: Vec<AdapterConfig>,
    pub pool_state: PoolStateConfig,
}

/// Data source configuration with support for different input types
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum DataSourceConfig {
    #[serde(rename = "grpc")]
    Grpc {
        endpoint: String,
        timeout_ms: u64,
        #[serde(default = "default_max_reconnect_attempts")]
        max_reconnect_attempts: u32,
        #[serde(default = "default_backoff_base_ms")]
        backoff_base_ms: u64,
    },
    #[serde(rename = "file")]
    File {
        path: String,
        #[serde(default)]
        replay_speed: Option<f64>, // None = as fast as possible
        #[serde(default = "default_preserve_timing")]
        preserve_timing: bool,
    },
}

/// Enhanced filtering configuration with multiple filter types
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IngestorFilterConfig {
    #[serde(default)]
    pub token_whitelist: Option<Vec<String>>,
    #[serde(default)]
    pub token_pairs: Option<Vec<(String, String)>>,
    #[serde(default)]
    pub min_liquidity: Option<String>, // Decimal as string
    #[serde(default)]
    pub dex_whitelist: Option<Vec<String>>,
    #[serde(default = "default_filter_enabled")]
    pub enabled: bool,
}

/// Performance and monitoring configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PerformanceConfig {
    #[serde(default = "default_max_events_per_batch")]
    pub max_events_per_batch: usize,
    #[serde(default = "default_batch_timeout_ms")]
    pub batch_timeout_ms: u64,
    #[serde(default = "default_channel_buffer_size")]
    pub channel_buffer_size: usize,
    #[serde(default = "default_latency_warning_threshold_ms")]
    pub latency_warning_threshold_ms: u64,
    #[serde(default = "default_metrics_enabled")]
    pub metrics_enabled: bool,
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
    
    // Warning system configuration
    #[serde(default = "default_warning_escalation_count")]
    pub warning_escalation_count: u32,
    #[serde(default = "default_critical_latency_multiplier")]
    pub critical_latency_multiplier: f64,
    #[serde(default = "default_warning_log_interval_seconds")]
    pub warning_log_interval_seconds: u64,

    // Backpressure handling configuration
    #[serde(default = "default_channel_send_timeout_ms")]
    pub channel_send_timeout_ms: u64,
    #[serde(default = "default_circuit_breaker_failure_threshold")]
    pub circuit_breaker_failure_threshold: u32,
    #[serde(default = "default_circuit_breaker_recovery_timeout_ms")]
    pub circuit_breaker_recovery_timeout_ms: u64,
    #[serde(default = "default_queue_depth_warning_threshold")]
    pub queue_depth_warning_threshold: f64,
    #[serde(default = "default_backpressure_retry_attempts")]
    pub backpressure_retry_attempts: u32,
    #[serde(default = "default_backpressure_retry_base_delay_ms")]
    pub backpressure_retry_base_delay_ms: u64,
    #[serde(default = "default_backpressure_retry_max_delay_ms")]
    pub backpressure_retry_max_delay_ms: u64,
}

/// DEX adapter configuration - aligned with existing DexConfig
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AdapterConfig {
    pub name: String,
    pub module_address: String,
    pub events: HashMap<String, String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub settings: HashMap<String, serde_yaml::Value>,
}

/// Pool state management configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PoolStateConfig {
    #[serde(default = "default_cache_ttl_seconds")]
    pub cache_ttl_seconds: u64,
    #[serde(default = "default_max_cache_size")]
    pub max_cache_size: usize,
    #[serde(default = "default_fetch_timeout_ms")]
    pub fetch_timeout_ms: u64,
    #[serde(default = "default_max_concurrent_fetches")]
    pub max_concurrent_fetches: usize,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,
}

// Default value functions
fn default_max_reconnect_attempts() -> u32 {
    3
}
fn default_backoff_base_ms() -> u64 {
    1000
}
fn default_preserve_timing() -> bool {
    true
}
fn default_filter_enabled() -> bool {
    true
}
fn default_max_events_per_batch() -> usize {
    100
}
fn default_batch_timeout_ms() -> u64 {
    100
}
fn default_channel_buffer_size() -> usize {
    1000
}
fn default_latency_warning_threshold_ms() -> u64 {
    100
}
fn default_metrics_enabled() -> bool {
    true
}
fn default_metrics_port() -> u16 {
    9090
}
fn default_cache_ttl_seconds() -> u64 {
    300
}
fn default_max_cache_size() -> usize {
    10000
}
fn default_fetch_timeout_ms() -> u64 {
    5000
}
fn default_max_concurrent_fetches() -> usize {
    10
}
fn default_retry_attempts() -> u32 {
    3
}

// Warning system defaults
fn default_warning_escalation_count() -> u32 {
    3
}

fn default_critical_latency_multiplier() -> f64 {
    2.0
}

fn default_warning_log_interval_seconds() -> u64 {
    60
}

// Backpressure handling defaults
fn default_channel_send_timeout_ms() -> u64 {
    5000
}

fn default_circuit_breaker_failure_threshold() -> u32 {
    5
}

fn default_circuit_breaker_recovery_timeout_ms() -> u64 {
    30000
}

fn default_queue_depth_warning_threshold() -> f64 {
    0.8
}

fn default_backpressure_retry_attempts() -> u32 {
    3
}

fn default_backpressure_retry_base_delay_ms() -> u64 {
    100
}

fn default_backpressure_retry_max_delay_ms() -> u64 {
    5000
}

// A serializable representation of the transaction stream config from the YAML.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct YamlTransactionStreamConfig {
    pub starting_version: Option<u64>,
    pub indexer_grpc_data_service_address: String,
    pub auth_token: String,
    pub request_name_header: String,
}

// The top-level configuration struct that maps directly to the YAML file.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub transaction_stream_config: YamlTransactionStreamConfig,
    pub market_data_config: MarketDataConfig,
    pub detector_config: DetectorConfig,
    /// Enhanced ingestor configuration (optional for backward compatibility)
    #[serde(default)]
    pub ingestor: Option<IngestorConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DetectorConfig {
    pub strategies: Vec<StrategyConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarketDataConfig {
    pub data_source: DataSource,
    pub filters: FilterConfig,
    pub dexs: Vec<DexConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum DataSource {
    #[serde(rename = "grpc")]
    Grpc,
    #[serde(rename = "file")]
    File { path: String, replay_speed: f64 },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "mode")]
pub enum FilterConfig {
    #[serde(rename = "token_pairs")]
    TokenPairs { token_pairs: Vec<(String, String)> },
    #[serde(rename = "token")]
    Token { token: String },
    #[serde(rename = "all")]
    All,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DexConfig {
    pub name: String,
    pub module_address: String,
    pub events: HashMap<String, String>,
    #[serde(default)]
    pub settings: HashMap<String, serde_yaml::Value>,
}

pub async fn load_config_from_path(path: &str) -> Result<Config, anyhow::Error> {
    let config_str = tokio::fs::read_to_string(path).await?;
    let mut config: Config = serde_yaml::from_str(&config_str)?;

    // Apply environment-specific overrides
    apply_environment_overrides(&mut config)?;

    // Validate configuration
    validate_config(&config)?;

    Ok(config)
}

/// Apply environment variable overrides to configuration
pub fn apply_environment_overrides(config: &mut Config) -> Result<(), anyhow::Error> {
    use std::env;

    // Override gRPC endpoint if environment variable is set
    if let Ok(endpoint) = env::var("APTOS_GRPC_ENDPOINT") {
        config
            .transaction_stream_config
            .indexer_grpc_data_service_address = endpoint;
    }

    // Override auth token if environment variable is set
    if let Ok(token) = env::var("APTOS_AUTH_TOKEN") {
        config.transaction_stream_config.auth_token = token;
    }

    // Override ingestor data source if environment variable is set
    if let Some(ref mut ingestor_config) = config.ingestor {
        if let Ok(data_source_type) = env::var("INGESTOR_DATA_SOURCE_TYPE") {
            match data_source_type.as_str() {
                "grpc" => {
                    let endpoint = env::var("INGESTOR_GRPC_ENDPOINT")
                        .unwrap_or_else(|_| "http://localhost:50051".to_string());
                    let timeout = env::var("INGESTOR_GRPC_TIMEOUT_MS")
                        .unwrap_or_else(|_| "30000".to_string())
                        .parse::<u64>()
                        .unwrap_or(30000);

                    ingestor_config.data_source = DataSourceConfig::Grpc {
                        endpoint,
                        timeout_ms: timeout,
                        max_reconnect_attempts: default_max_reconnect_attempts(),
                        backoff_base_ms: default_backoff_base_ms(),
                    };
                }
                "file" => {
                    let path = env::var("INGESTOR_FILE_PATH")
                        .unwrap_or_else(|_| "recordings/sample.pb".to_string());
                    let speed = env::var("INGESTOR_REPLAY_SPEED")
                        .ok()
                        .and_then(|s| s.parse::<f64>().ok());

                    ingestor_config.data_source = DataSourceConfig::File {
                        path,
                        replay_speed: speed,
                        preserve_timing: default_preserve_timing(),
                    };
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Invalid INGESTOR_DATA_SOURCE_TYPE: {}",
                        data_source_type
                    ));
                }
            }
        }

        // Override metrics settings
        if let Ok(metrics_enabled) = env::var("INGESTOR_METRICS_ENABLED") {
            ingestor_config.performance.metrics_enabled = metrics_enabled.parse().unwrap_or(true);
        }

        if let Ok(metrics_port) = env::var("INGESTOR_METRICS_PORT") {
            ingestor_config.performance.metrics_port = metrics_port.parse().unwrap_or(9090);
        }
    }

    Ok(())
}

/// Validate configuration for correctness and consistency
pub fn validate_config(config: &Config) -> Result<(), anyhow::Error> {
    // Validate transaction stream config
    if config
        .transaction_stream_config
        .indexer_grpc_data_service_address
        .is_empty()
    {
        return Err(anyhow::anyhow!(
            "Transaction stream gRPC address cannot be empty"
        ));
    }

    if config.transaction_stream_config.auth_token.is_empty() {
        return Err(anyhow::anyhow!("Auth token cannot be empty"));
    }

    // Validate ingestor config if present
    if let Some(ref ingestor_config) = config.ingestor {
        validate_ingestor_config(ingestor_config)?;
    }

    Ok(())
}

/// Validate ingestor-specific configuration
pub fn validate_ingestor_config(config: &IngestorConfig) -> Result<(), anyhow::Error> {
    // Validate data source configuration
    match &config.data_source {
        DataSourceConfig::Grpc {
            endpoint,
            timeout_ms,
            ..
        } => {
            if endpoint.is_empty() {
                return Err(anyhow::anyhow!("gRPC endpoint cannot be empty"));
            }
            if *timeout_ms == 0 {
                return Err(anyhow::anyhow!("gRPC timeout must be greater than 0"));
            }
        }
        DataSourceConfig::File {
            path, replay_speed, ..
        } => {
            if path.is_empty() {
                return Err(anyhow::anyhow!("File path cannot be empty"));
            }
            if let Some(speed) = replay_speed {
                if *speed < 0.0 {
                    return Err(anyhow::anyhow!("Replay speed cannot be negative"));
                }
            }
        }
    }

    // Validate performance configuration
    if config.performance.max_events_per_batch == 0 {
        return Err(anyhow::anyhow!(
            "max_events_per_batch must be greater than 0"
        ));
    }

    if config.performance.channel_buffer_size == 0 {
        return Err(anyhow::anyhow!(
            "channel_buffer_size must be greater than 0"
        ));
    }

    // Validate backpressure configuration
    if config.performance.channel_send_timeout_ms == 0 {
        return Err(anyhow::anyhow!(
            "channel_send_timeout_ms must be greater than 0"
        ));
    }

    if config.performance.circuit_breaker_failure_threshold == 0 {
        return Err(anyhow::anyhow!(
            "circuit_breaker_failure_threshold must be greater than 0"
        ));
    }

    if config.performance.circuit_breaker_recovery_timeout_ms == 0 {
        return Err(anyhow::anyhow!(
            "circuit_breaker_recovery_timeout_ms must be greater than 0"
        ));
    }

    if config.performance.queue_depth_warning_threshold <= 0.0 || config.performance.queue_depth_warning_threshold > 1.0 {
        return Err(anyhow::anyhow!(
            "queue_depth_warning_threshold must be between 0.0 and 1.0"
        ));
    }

    if config.performance.backpressure_retry_base_delay_ms == 0 {
        return Err(anyhow::anyhow!(
            "backpressure_retry_base_delay_ms must be greater than 0"
        ));
    }

    if config.performance.backpressure_retry_max_delay_ms < config.performance.backpressure_retry_base_delay_ms {
        return Err(anyhow::anyhow!(
            "backpressure_retry_max_delay_ms must be greater than or equal to backpressure_retry_base_delay_ms"
        ));
    }

    // Validate adapter configurations
    for adapter in &config.adapters {
        if adapter.name.is_empty() {
            return Err(anyhow::anyhow!("Adapter name cannot be empty"));
        }
        if adapter.module_address.is_empty() {
            return Err(anyhow::anyhow!("Adapter module address cannot be empty"));
        }
    }

    // Validate pool state configuration
    if config.pool_state.max_cache_size == 0 {
        return Err(anyhow::anyhow!("max_cache_size must be greater than 0"));
    }

    if config.pool_state.fetch_timeout_ms == 0 {
        return Err(anyhow::anyhow!("fetch_timeout_ms must be greater than 0"));
    }

    Ok(())
}

/// Create a default IngestorConfig for testing or fallback scenarios
pub fn default_ingestor_config() -> IngestorConfig {
    IngestorConfig {
        data_source: DataSourceConfig::Grpc {
            endpoint: "http://localhost:50051".to_string(),
            timeout_ms: 30000,
            max_reconnect_attempts: default_max_reconnect_attempts(),
            backoff_base_ms: default_backoff_base_ms(),
        },
        filters: IngestorFilterConfig {
            token_whitelist: None,
            token_pairs: None,
            min_liquidity: None,
            dex_whitelist: None,
            enabled: default_filter_enabled(),
        },
        performance: PerformanceConfig {
            max_events_per_batch: default_max_events_per_batch(),
            batch_timeout_ms: default_batch_timeout_ms(),
            channel_buffer_size: default_channel_buffer_size(),
            latency_warning_threshold_ms: default_latency_warning_threshold_ms(),
            metrics_enabled: default_metrics_enabled(),
            metrics_port: default_metrics_port(),
            warning_escalation_count: default_warning_escalation_count(),
            critical_latency_multiplier: default_critical_latency_multiplier(),
            warning_log_interval_seconds: default_warning_log_interval_seconds(),
            channel_send_timeout_ms: default_channel_send_timeout_ms(),
            circuit_breaker_failure_threshold: default_circuit_breaker_failure_threshold(),
            circuit_breaker_recovery_timeout_ms: default_circuit_breaker_recovery_timeout_ms(),
            queue_depth_warning_threshold: default_queue_depth_warning_threshold(),
            backpressure_retry_attempts: default_backpressure_retry_attempts(),
            backpressure_retry_base_delay_ms: default_backpressure_retry_base_delay_ms(),
            backpressure_retry_max_delay_ms: default_backpressure_retry_max_delay_ms(),
        },
        adapters: vec![],
        pool_state: PoolStateConfig {
            cache_ttl_seconds: default_cache_ttl_seconds(),
            max_cache_size: default_max_cache_size(),
            fetch_timeout_ms: default_fetch_timeout_ms(),
            max_concurrent_fetches: default_max_concurrent_fetches(),
            retry_attempts: default_retry_attempts(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_default_ingestor_config() {
        let config = default_ingestor_config();

        // Verify data source defaults
        match config.data_source {
            DataSourceConfig::Grpc {
                endpoint,
                timeout_ms,
                ..
            } => {
                assert_eq!(endpoint, "http://localhost:50051");
                assert_eq!(timeout_ms, 30000);
            }
            _ => panic!("Expected gRPC data source"),
        }

        // Verify performance defaults
        assert_eq!(config.performance.max_events_per_batch, 100);
        assert_eq!(config.performance.batch_timeout_ms, 100);
        assert_eq!(config.performance.channel_buffer_size, 1000);
        assert!(config.performance.metrics_enabled);
        assert_eq!(config.performance.metrics_port, 9090);

        // Verify pool state defaults
        assert_eq!(config.pool_state.cache_ttl_seconds, 300);
        assert_eq!(config.pool_state.max_cache_size, 10000);
        assert_eq!(config.pool_state.fetch_timeout_ms, 5000);
    }

    #[test]
    fn test_validate_ingestor_config_success() {
        let config = default_ingestor_config();
        assert!(validate_ingestor_config(&config).is_ok());
    }

    #[test]
    fn test_validate_ingestor_config_empty_grpc_endpoint() {
        let mut config = default_ingestor_config();
        config.data_source = DataSourceConfig::Grpc {
            endpoint: "".to_string(),
            timeout_ms: 30000,
            max_reconnect_attempts: 3,
            backoff_base_ms: 1000,
        };

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("gRPC endpoint cannot be empty")
        );
    }

    #[test]
    fn test_validate_ingestor_config_zero_timeout() {
        let mut config = default_ingestor_config();
        config.data_source = DataSourceConfig::Grpc {
            endpoint: "http://localhost:50051".to_string(),
            timeout_ms: 0,
            max_reconnect_attempts: 3,
            backoff_base_ms: 1000,
        };

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("gRPC timeout must be greater than 0")
        );
    }

    #[test]
    fn test_validate_ingestor_config_empty_file_path() {
        let mut config = default_ingestor_config();
        config.data_source = DataSourceConfig::File {
            path: "".to_string(),
            replay_speed: Some(1.0),
            preserve_timing: true,
        };

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("File path cannot be empty")
        );
    }

    #[test]
    fn test_validate_ingestor_config_negative_replay_speed() {
        let mut config = default_ingestor_config();
        config.data_source = DataSourceConfig::File {
            path: "test.pb".to_string(),
            replay_speed: Some(-1.0),
            preserve_timing: true,
        };

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Replay speed cannot be negative")
        );
    }

    #[test]
    fn test_validate_ingestor_config_zero_events_per_batch() {
        let mut config = default_ingestor_config();
        config.performance.max_events_per_batch = 0;

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("max_events_per_batch must be greater than 0")
        );
    }

    #[test]
    fn test_validate_ingestor_config_zero_buffer_size() {
        let mut config = default_ingestor_config();
        config.performance.channel_buffer_size = 0;

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("channel_buffer_size must be greater than 0")
        );
    }

    #[test]
    fn test_validate_ingestor_config_empty_adapter_name() {
        let mut config = default_ingestor_config();
        config.adapters.push(AdapterConfig {
            name: "".to_string(),
            module_address: "0x1".to_string(),
            events: HashMap::new(),
            enabled: true,
            settings: HashMap::new(),
        });

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Adapter name cannot be empty")
        );
    }

    #[test]
    fn test_validate_ingestor_config_empty_adapter_address() {
        let mut config = default_ingestor_config();
        config.adapters.push(AdapterConfig {
            name: "test_adapter".to_string(),
            module_address: "".to_string(),
            events: HashMap::new(),
            enabled: true,
            settings: HashMap::new(),
        });

        let result = validate_ingestor_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Adapter module address cannot be empty")
        );
    }

    #[test]
    fn test_apply_environment_overrides_grpc_endpoint() {
        unsafe {
            env::set_var("APTOS_GRPC_ENDPOINT", "http://testnet:50051");
        }

        let mut config = Config {
            transaction_stream_config: YamlTransactionStreamConfig {
                starting_version: None,
                indexer_grpc_data_service_address: "http://localhost:50051".to_string(),
                auth_token: "test_token".to_string(),
                request_name_header: "test_header".to_string(),
            },
            market_data_config: MarketDataConfig {
                data_source: DataSource::Grpc,
                filters: FilterConfig::All,
                dexs: vec![],
            },
            detector_config: DetectorConfig { strategies: vec![] },
            ingestor: None,
        };

        apply_environment_overrides(&mut config).unwrap();
        assert_eq!(
            config
                .transaction_stream_config
                .indexer_grpc_data_service_address,
            "http://testnet:50051"
        );

        unsafe {
            env::remove_var("APTOS_GRPC_ENDPOINT");
        }
    }

    #[test]
    fn test_apply_environment_overrides_auth_token() {
        unsafe {
            env::set_var("APTOS_AUTH_TOKEN", "new_test_token");
        }

        let mut config = Config {
            transaction_stream_config: YamlTransactionStreamConfig {
                starting_version: None,
                indexer_grpc_data_service_address: "http://localhost:50051".to_string(),
                auth_token: "old_token".to_string(),
                request_name_header: "test_header".to_string(),
            },
            market_data_config: MarketDataConfig {
                data_source: DataSource::Grpc,
                filters: FilterConfig::All,
                dexs: vec![],
            },
            detector_config: DetectorConfig { strategies: vec![] },
            ingestor: None,
        };

        apply_environment_overrides(&mut config).unwrap();
        assert_eq!(
            config.transaction_stream_config.auth_token,
            "new_test_token"
        );

        unsafe {
            env::remove_var("APTOS_AUTH_TOKEN");
        }
    }

    #[test]
    fn test_apply_environment_overrides_ingestor_data_source() {
        unsafe {
            env::set_var("INGESTOR_DATA_SOURCE_TYPE", "file");
            env::set_var("INGESTOR_FILE_PATH", "test_recording.pb");
            env::set_var("INGESTOR_REPLAY_SPEED", "2.0");
        }

        let mut config = Config {
            transaction_stream_config: YamlTransactionStreamConfig {
                starting_version: None,
                indexer_grpc_data_service_address: "http://localhost:50051".to_string(),
                auth_token: "test_token".to_string(),
                request_name_header: "test_header".to_string(),
            },
            market_data_config: MarketDataConfig {
                data_source: DataSource::Grpc,
                filters: FilterConfig::All,
                dexs: vec![],
            },
            detector_config: DetectorConfig { strategies: vec![] },
            ingestor: Some(default_ingestor_config()),
        };

        apply_environment_overrides(&mut config).unwrap();

        if let Some(ref ingestor_config) = config.ingestor {
            match &ingestor_config.data_source {
                DataSourceConfig::File {
                    path, replay_speed, ..
                } => {
                    assert_eq!(path, "test_recording.pb");
                    assert_eq!(replay_speed, &Some(2.0));
                }
                _ => panic!("Expected file data source"),
            }
        } else {
            panic!("Expected ingestor config");
        }

        unsafe {
            env::remove_var("INGESTOR_DATA_SOURCE_TYPE");
            env::remove_var("INGESTOR_FILE_PATH");
            env::remove_var("INGESTOR_REPLAY_SPEED");
        }
    }

    #[test]
    fn test_yaml_deserialization() {
        let yaml_content = r#"
transaction_stream_config:
  starting_version: 12345
  indexer_grpc_data_service_address: "http://localhost:50051"
  auth_token: "test_token"
  request_name_header: "test_header"

market_data_config:
  data_source:
    type: grpc
  filters:
    mode: all
  dexs: []

detector_config:
  strategies: []

ingestor:
  data_source:
    type: grpc
    endpoint: "http://localhost:50051"
    timeout_ms: 30000
  filters:
    enabled: true
    token_whitelist: ["USDC", "APT"]
  performance:
    max_events_per_batch: 200
    batch_timeout_ms: 50
    metrics_enabled: true
    metrics_port: 9091
  adapters:
    - name: "hyperion"
      module_address: "0x1"
      events: {}
      enabled: true
  pool_state:
    cache_ttl_seconds: 600
    max_cache_size: 5000
"#;

        let config: Config = serde_yaml::from_str(yaml_content).unwrap();

        // Verify basic config
        assert_eq!(
            config.transaction_stream_config.starting_version,
            Some(12345)
        );
        assert_eq!(
            config
                .transaction_stream_config
                .indexer_grpc_data_service_address,
            "http://localhost:50051"
        );

        // Verify ingestor config was parsed
        assert!(config.ingestor.is_some());
        let ingestor_config = config.ingestor.unwrap();

        // Verify data source
        match ingestor_config.data_source {
            DataSourceConfig::Grpc {
                endpoint,
                timeout_ms,
                ..
            } => {
                assert_eq!(endpoint, "http://localhost:50051");
                assert_eq!(timeout_ms, 30000);
            }
            _ => panic!("Expected gRPC data source"),
        }

        // Verify filters
        assert!(ingestor_config.filters.enabled);
        assert_eq!(
            ingestor_config.filters.token_whitelist,
            Some(vec!["USDC".to_string(), "APT".to_string()])
        );

        // Verify performance settings
        assert_eq!(ingestor_config.performance.max_events_per_batch, 200);
        assert_eq!(ingestor_config.performance.batch_timeout_ms, 50);
        assert_eq!(ingestor_config.performance.metrics_port, 9091);

        // Verify adapters
        assert_eq!(ingestor_config.adapters.len(), 1);
        assert_eq!(ingestor_config.adapters[0].name, "hyperion");
        assert_eq!(ingestor_config.adapters[0].module_address, "0x1");
        assert!(ingestor_config.adapters[0].enabled);

        // Verify pool state
        assert_eq!(ingestor_config.pool_state.cache_ttl_seconds, 600);
        assert_eq!(ingestor_config.pool_state.max_cache_size, 5000);
    }
}
