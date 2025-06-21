use super::{ArbitrageStrategy, TriangularConfig};
use crate::graph::PriceGraphView;
use anyhow::Result;
use async_trait::async_trait;
use common::types::{ArbitrageOpportunity, GraphView};

/// Strategy for 3-node (triangular) arbitrage paths.
#[derive(Clone)]
pub struct TriangularArbitrage {
    config: TriangularConfig,
}

impl TriangularArbitrage {
    pub fn new(config: TriangularConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ArbitrageStrategy for TriangularArbitrage {
    fn name(&self) -> &str {
        "triangular_arbitrage"
    }

    fn required_graph_view(&self) -> GraphView {
        // Needs full graph for 3-node cycles
        GraphView::All
    }

    async fn detect_opportunities(
        &self,
        graph: &PriceGraphView,
        block_number: u64,
    ) -> Result<Vec<ArbitrageOpportunity>> {
        // TODO: Implement triangular arbitrage detection.
        Ok(Vec::new())
    }

    fn clone_dyn(&self) -> Box<dyn ArbitrageStrategy> {
        Box::new(self.clone())
    }
}
