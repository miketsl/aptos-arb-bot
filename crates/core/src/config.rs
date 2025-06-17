use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BotConfig {
    pub dexes: Vec<DexConfig>,
    pub detector: DetectorConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DexConfig {
    pub name: String,
    pub module_addr: String,
    pub grpc_indexer: Option<String>,
    pub grpc_auth_token: Option<String>, // For Aptos indexer stream
    pub starting_version: Option<u64>,  // For Aptos indexer stream
    pub fullnode_rpc: String,
    pub pairs: Vec<String>,
    pub relevant_event_types: Option<Vec<String>>, // For Hyperion adapter
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DetectorConfig {
    pub interval_ms: u64,
    pub min_profit_pct: f64,
    pub allowed_pairs: Option<HashSet<String>>,
}

impl BotConfig {
    /// Load configuration from a YAML file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| ConfigError::IoError(e))?;
        
        let mut config: BotConfig = serde_yaml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(e.to_string()))?;
        
        // Auto-populate allowed_pairs from dex configs if not set
        if config.detector.allowed_pairs.is_none() {
            let mut pairs = HashSet::new();
            for dex in &config.dexes {
                for pair in &dex.pairs {
                    pairs.insert(pair.clone());
                }
            }
            config.detector.allowed_pairs = Some(pairs);
        }
        
        Ok(config)
    }
    
    /// Save configuration to a YAML file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let content = serde_yaml::to_string(self)
            .map_err(|e| ConfigError::ParseError(e.to_string()))?;
        
        std::fs::write(path.as_ref(), content)
            .map_err(|e| ConfigError::IoError(e))?;
        
        Ok(())
    }
    
    /// Get DEX configuration by name
    pub fn get_dex_config(&self, name: &str) -> Option<&DexConfig> {
        self.dexes.iter().find(|dex| dex.name == name)
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.dexes.is_empty() {
            return Err(ConfigError::ValidationError("No DEXes configured".to_string()));
        }
        
        for dex in &self.dexes {
            if dex.name.is_empty() {
                return Err(ConfigError::ValidationError("DEX name cannot be empty".to_string()));
            }
            
            if dex.module_addr.is_empty() {
                return Err(ConfigError::ValidationError(format!("Module address for DEX '{}' cannot be empty", dex.name)));
            }
            
            if dex.fullnode_rpc.is_empty() {
                return Err(ConfigError::ValidationError(format!("Fullnode RPC for DEX '{}' cannot be empty", dex.name)));
            }
            
            if dex.pairs.is_empty() {
                return Err(ConfigError::ValidationError(format!("No pairs configured for DEX '{}'", dex.name)));
            }
        }
        
        if self.detector.interval_ms == 0 {
            return Err(ConfigError::ValidationError("Detector interval must be greater than 0".to_string()));
        }
        
        if self.detector.min_profit_pct < 0.0 {
            return Err(ConfigError::ValidationError("Minimum profit percentage cannot be negative".to_string()));
        }
        
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Parse error: {0}")]
    ParseError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    
    fn create_test_config() -> BotConfig {
        BotConfig {
            dexes: vec![
                DexConfig {
                    name: "hyperion".to_string(),
                    module_addr: "0xHYP".to_string(),
                    grpc_indexer: Some("https://indexer.mainnet.aptoslabs.com:50051".to_string()),
                    grpc_auth_token: Some("YOUR_API_KEY_HERE".to_string()),
                    starting_version: Some(0),
                    fullnode_rpc: "https://fullnode.mainnet.aptoslabs.com".to_string(),
                    pairs: vec!["APT/USDC".to_string(), "APT/USDT".to_string()],
                    relevant_event_types: Some(vec!["0xHYP::dex::SwapEvent".to_string()]),
                },
                DexConfig {
                    name: "thala".to_string(),
                    module_addr: "0xTHL".to_string(),
                    grpc_indexer: Some("https://indexer.mainnet.aptoslabs.com:50051".to_string()),
                    grpc_auth_token: Some("YOUR_API_KEY_HERE".to_string()),
                    starting_version: Some(0),
                    fullnode_rpc: "https://fullnode.mainnet.aptoslabs.com".to_string(),
                    pairs: vec!["APT/USDC".to_string(), "APT/USDT".to_string()],
                    relevant_event_types: Some(vec!["0xTHL::amm::SwapEvent".to_string()]),
                },
            ],
            detector: DetectorConfig {
                interval_ms: 500,
                min_profit_pct: 0.01,
                allowed_pairs: None, // Will be auto-populated
            },
        }
    }
    
    #[test]
    fn test_config_save_and_load() {
        let config = create_test_config();
        let temp_file = NamedTempFile::new().unwrap();
        
        // Save config
        config.save(temp_file.path()).unwrap();
        
        // Load config
        let loaded_config = BotConfig::load(temp_file.path()).unwrap();
        
        assert_eq!(loaded_config.dexes.len(), 2);
        assert_eq!(loaded_config.dexes[0].name, "hyperion");
        assert_eq!(loaded_config.dexes[1].name, "thala");
        assert_eq!(loaded_config.detector.interval_ms, 500);
        assert_eq!(loaded_config.detector.min_profit_pct, 0.01);
        
        // Check auto-populated allowed_pairs
        let allowed_pairs = loaded_config.detector.allowed_pairs.unwrap();
        assert!(allowed_pairs.contains("APT/USDC"));
        assert!(allowed_pairs.contains("APT/USDT"));
    }
    
    #[test]
    fn test_config_validation() {
        let mut config = create_test_config();
        
        // Valid config should pass
        config.validate().unwrap();
        
        // Empty DEXes should fail
        config.dexes.clear();
        assert!(config.validate().is_err());
        
        // Reset and test invalid interval
        config = create_test_config();
        config.detector.interval_ms = 0;
        assert!(config.validate().is_err());
        
        // Reset and test negative profit
        config = create_test_config();
        config.detector.min_profit_pct = -0.1;
        assert!(config.validate().is_err());
    }
    
    #[test]
    fn test_get_dex_config() {
        let config = create_test_config();
        
        let hyperion_config = config.get_dex_config("hyperion").unwrap();
        assert_eq!(hyperion_config.name, "hyperion");
        assert_eq!(hyperion_config.module_addr, "0xHYP");
        
        let thala_config = config.get_dex_config("thala").unwrap();
        assert_eq!(thala_config.name, "thala");
        assert_eq!(thala_config.module_addr, "0xTHL");
        
        assert!(config.get_dex_config("nonexistent").is_none());
    }
}
