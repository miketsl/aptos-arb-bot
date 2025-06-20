use crate::exchange_const::Exchange;
use anyhow::Result;
use async_trait::async_trait;
use common::types::{CycleEval, PathQuote};

/// Represents a potentially profitable arbitrage opportunity, combining the trade path
/// and the financial evaluation.
#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    pub path_quote: PathQuote<Exchange>,
    pub cycle_eval: CycleEval,
}

/// A trait for a service that assesses the risk of an arbitrage opportunity.
#[async_trait]
pub trait IsRiskManager: Send + Sync {
    /// Assesses the risk of an arbitrage opportunity.
    /// Returns `true` if the opportunity is safe to execute, `false` otherwise.
    async fn assess_risk(&self, opportunity: &ArbitrageOpportunity) -> Result<bool>;
}

/// A trait for a service that executes a trade.
#[async_trait]
pub trait IsExecutor: Send + Sync {
    /// Executes a trade for the given arbitrage opportunity.
    async fn execute_trade(&self, opportunity: &ArbitrageOpportunity) -> Result<()>;
}

/// Dummy implementations for testing
#[cfg(test)]
pub mod test_implementations {
    use super::*;

    /// A dummy risk manager that always approves trades.
    #[derive(Debug, Clone, Default)]
    pub struct DummyRiskManager;

    #[async_trait]
    impl IsRiskManager for DummyRiskManager {
        async fn assess_risk(&self, _opportunity: &ArbitrageOpportunity) -> Result<bool> {
            Ok(true)
        }
    }

    /// A dummy executor that always succeeds.
    #[derive(Debug, Clone, Default)]
    pub struct DummyExecutor;

    #[async_trait]
    impl IsExecutor for DummyExecutor {
        async fn execute_trade(&self, _opportunity: &ArbitrageOpportunity) -> Result<()> {
            Ok(())
        }
    }
}
