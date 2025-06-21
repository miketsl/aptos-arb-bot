use crate::graph::PriceGraphView;
use anyhow::Result;
use async_trait::async_trait;
use common::types::{ArbitrageOpportunity, GraphView};
use serde::Deserialize;

pub mod cross_dex;

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrategyConfig {
    CrossDex(CrossDexConfig),
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct CrossDexConfig {
    // Configuration specific to the Cross-DEX strategy, if any.
    // For example, a list of pairs to monitor.
    // For now, we'll leave it empty.
}

#[async_trait]
pub trait ArbitrageStrategy: Send + Sync {
    fn name(&self) -> &str;
    fn required_graph_view(&self) -> GraphView;
    async fn detect_opportunities(
        &self,
        graph: &PriceGraphView,
        block_number: u64,
    ) -> Result<Vec<ArbitrageOpportunity>>;
    fn clone_dyn(&self) -> Box<dyn ArbitrageStrategy>;
}

impl Clone for Box<dyn ArbitrageStrategy> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

/// Creates a new strategy instance from its configuration.
pub fn create_strategy(config: &StrategyConfig) -> Result<Box<dyn ArbitrageStrategy>> {
    match config {
        StrategyConfig::CrossDex(config) => {
            Ok(Box::new(cross_dex::CrossDexArbitrage::new(config.clone())))
        }
    }
}
