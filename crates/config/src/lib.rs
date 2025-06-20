use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    let config: Config = serde_yaml::from_str(&config_str)?;
    Ok(config)
}
