use super::{ArbitrageStrategy, MultiHopConfig};
use crate::graph::PriceGraphView;
use anyhow::Result;
use async_trait::async_trait;
use common::types::{ArbitrageOpportunity, GraphView};

/// Strategy for multi-hop arbitrage paths.
#[derive(Clone)]
pub struct MultiHopArbitrage {
    config: MultiHopConfig,
}

impl MultiHopArbitrage {
    pub fn new(config: MultiHopConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ArbitrageStrategy for MultiHopArbitrage {
    fn name(&self) -> &str {
        "multi_hop_arbitrage"
    }

    fn required_graph_view(&self) -> GraphView {
        // Needs full graph for multi-hop search
        GraphView::All
    }

    async fn detect_opportunities(
        &self,
        graph: &PriceGraphView,
        block_number: u64,
    ) -> Result<Vec<ArbitrageOpportunity>> {
        // TODO: Implement multi-hop arbitrage detection.
        Ok(Vec::new())
    }

    fn clone_dyn(&self) -> Box<dyn ArbitrageStrategy> {
        Box::new(self.clone())
    }
}
