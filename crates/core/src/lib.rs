//! Orchestration, config, and dependency injection.

pub mod config;
pub mod risk_manager;

pub use config::{BotConfig, ConfigError, DexConfig, DetectorConfig};
pub use risk_manager::{ConservativeRiskManager, DummyRiskManager};

pub fn init() {}
