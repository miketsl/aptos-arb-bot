use crate::graph::PriceGraphView;
use anyhow::Result;
use async_trait::async_trait;
use common::types::{ArbitrageOpportunity, GraphView};
use serde::Deserialize;

pub mod cross_dex;
pub mod multi_hop;
pub mod triangular;

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrategyConfig {
    CrossDex(CrossDexConfig),
    Triangular(TriangularConfig),
    MultiHop(MultiHopConfig),
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct CrossDexConfig {
    // Configuration specific to the Cross-DEX strategy, if any.
    // For example, a list of pairs to monitor.
    // For now, we'll leave it empty.
}

/// Configuration for the triangular arbitrage strategy.
#[derive(Debug, Deserialize, Clone)]
pub struct TriangularConfig {
    pub max_path_length: usize,
    pub target_dex: Option<String>,
}

/// Configuration for the multi-hop arbitrage strategy.
#[derive(Debug, Deserialize, Clone)]
pub struct MultiHopConfig {
    pub max_hops: usize,
    pub min_liquidity: rust_decimal::Decimal,
    pub enable_cross_dex: bool,
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

    /// Returns the graph views to run on this strategy, given the set of updated pairs.
    /// By default, strategies run on their `required_graph_view()` only once per block.
    fn incremental_views(
        &self,
        _updated: &std::collections::HashSet<common::types::TradingPair>,
    ) -> Vec<GraphView> {
        vec![self.required_graph_view()]
    }
}

impl Clone for Box<dyn ArbitrageStrategy> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

/// Creates a new strategy instance from its configuration.
pub fn create_strategy(config: &StrategyConfig) -> Result<Box<dyn ArbitrageStrategy>> {
    match config {
        StrategyConfig::CrossDex(cfg) => {
            Ok(Box::new(cross_dex::CrossDexArbitrage::new(cfg.clone())))
        }
        StrategyConfig::Triangular(cfg) => {
            Ok(Box::new(triangular::TriangularArbitrage::new(cfg.clone())))
        }
        StrategyConfig::MultiHop(cfg) => {
            Ok(Box::new(multi_hop::MultiHopArbitrage::new(cfg.clone())))
        }
    }
}
