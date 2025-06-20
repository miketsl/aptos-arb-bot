//! Risk management implementations.

use anyhow::Result;
use async_trait::async_trait;
use detector::traits::{ArbitrageOpportunity, IsRiskManager};
use rust_decimal::Decimal;

/// A dummy risk manager that always approves trades.
/// Used for testing and development purposes.
#[derive(Debug, Clone)]
pub struct DummyRiskManager {
    /// Minimum net profit threshold to approve a trade.
    pub min_net_profit: Decimal,
}

impl DummyRiskManager {
    /// Creates a new dummy risk manager.
    pub fn new() -> Self {
        Self {
            min_net_profit: Decimal::new(1, 4), // 0.0001
        }
    }

    /// Creates a new dummy risk manager with a specified minimum profit.
    pub fn with_min_profit(min_net_profit: Decimal) -> Self {
        Self { min_net_profit }
    }
}

impl Default for DummyRiskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IsRiskManager for DummyRiskManager {
    async fn assess_risk(&self, opportunity: &ArbitrageOpportunity) -> Result<bool> {
        // Simple risk assessment: approve if net profit exceeds threshold
        let approved = opportunity.cycle_eval.net_profit >= self.min_net_profit;

        if approved {
            log::info!(
                "Risk assessment APPROVED: net_profit = {} >= {}",
                opportunity.cycle_eval.net_profit,
                self.min_net_profit
            );
        } else {
            log::warn!(
                "Risk assessment REJECTED: net_profit = {} < {}",
                opportunity.cycle_eval.net_profit,
                self.min_net_profit
            );
        }

        Ok(approved)
    }
}

/// A conservative risk manager that rejects all trades.
/// Used for testing fail-safe behavior.
#[derive(Debug, Clone, Default)]
pub struct ConservativeRiskManager;

impl ConservativeRiskManager {
    /// Creates a new conservative risk manager.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl IsRiskManager for ConservativeRiskManager {
    async fn assess_risk(&self, _opportunity: &ArbitrageOpportunity) -> Result<bool> {
        log::warn!("Conservative risk manager rejecting all trades");
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{Asset, CycleEval, PathQuote, Quantity};
    use dex_adapter_trait::Exchange;
    use rust_decimal_macros::dec;

    fn create_test_opportunity(net_profit: Decimal) -> ArbitrageOpportunity {
        ArbitrageOpportunity {
            path_quote: PathQuote {
                path: vec![
                    (Asset::from("USDC"), Exchange::Tapp),
                    (Asset::from("APT"), Exchange::Tapp),
                ],
                amount_in: Quantity(dec!(100)),
                amount_out: Quantity(dec!(105)),
                profit_pct: 0.05,
            },
            cycle_eval: CycleEval {
                gross_profit: dec!(5),
                gas_estimate: 1000,
                gas_unit_price: dec!(0.001),
                net_profit,
            },
        }
    }

    #[tokio::test]
    async fn test_dummy_risk_manager_approval() {
        let manager = DummyRiskManager::new();
        let opportunity = create_test_opportunity(dec!(0.001)); // Above threshold

        let result = manager.assess_risk(&opportunity).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_dummy_risk_manager_rejection() {
        let manager = DummyRiskManager::new();
        let opportunity = create_test_opportunity(dec!(0.00001)); // Below threshold

        let result = manager.assess_risk(&opportunity).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_conservative_risk_manager() {
        let manager = ConservativeRiskManager::new();
        let opportunity = create_test_opportunity(dec!(1000)); // High profit, still rejected

        let result = manager.assess_risk(&opportunity).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
