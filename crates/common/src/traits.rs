//! Shared traits for services in the arbitrage bot.

use crate::types::ArbitrageOpportunity;
use anyhow::Result;
use async_trait::async_trait;

/// A trait for risk management services.
#[async_trait]
pub trait IsRiskManager: Send + Sync {
    /// Assesses the risk of an arbitrage opportunity.
    async fn assess_risk(&self, opportunity: &ArbitrageOpportunity) -> Result<bool>;
}

/// A trait for trade execution services.
#[async_trait]
pub trait IsExecutor: Send + Sync {
    /// Executes a trade based on an arbitrage opportunity.
    async fn execute_trade(&self, opportunity: &ArbitrageOpportunity) -> Result<()>;
}
