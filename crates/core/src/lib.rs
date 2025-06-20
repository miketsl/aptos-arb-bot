//! Orchestration, config, and dependency injection.

pub mod risk_manager;

pub use risk_manager::{ConservativeRiskManager, DummyRiskManager};

pub fn init() {}
