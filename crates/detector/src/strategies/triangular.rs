use super::{ArbitrageStrategy, TriangularConfig};
use crate::graph::PriceGraphView;
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use common::types::Quantity;
use common::types::{ArbitrageOpportunity, GraphView};
use rust_decimal::Decimal;
use uuid::Uuid;

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
        let mut opportunities = Vec::with_capacity(graph.graph.edge_count());
        let one = Quantity(Decimal::ONE);
        let mut path_buf = Vec::with_capacity(3);
        for a in graph.graph.nodes() {
            for b in graph.graph.neighbors(a) {
                for c in graph.graph.neighbors(b) {
                    // must form a cycle back to a
                    if c == a || !graph.graph.contains_edge(c, a) {
                        continue;
                    }
                    // fetch the three edges
                    let e_ab = graph.graph.edge_weight(a, b).unwrap();
                    let e_bc = graph.graph.edge_weight(b, c).unwrap();
                    let e_ca = graph.graph.edge_weight(c, a).unwrap();
                    // apply optional target-dex filter
                    if let Some(target) = &self.config.target_dex {
                        if ![&e_ab.exchange, &e_bc.exchange, &e_ca.exchange]
                            .iter()
                            .map(|ex| ex.to_string())
                            .any(|e| &e == target)
                        {
                            continue;
                        }
                    }
                    // simulate trades: A->B, B->C, C->A
                    if let Some(out_ab) = e_ab.quote(&one, &graph.asset_mapping[&a]) {
                        if let Some(out_bc) = e_bc.quote(&out_ab, &graph.asset_mapping[&b]) {
                            if let Some(out_ca) = e_ca.quote(&out_bc, &graph.asset_mapping[&c]) {
                                let profit = out_ca.0 - one.0;
                                if profit > Decimal::ZERO {
                                    path_buf.clear();
                                    path_buf.push(e_ab.to_serializable());
                                    path_buf.push(e_bc.to_serializable());
                                    path_buf.push(e_ca.to_serializable());
                                    opportunities.push(ArbitrageOpportunity {
                                        id: Uuid::new_v4(),
                                        strategy: self.name().to_string(),
                                        path: path_buf.clone(),
                                        expected_profit: profit,
                                        input_amount: one.0,
                                        gas_estimate: 0,
                                        block_number,
                                        timestamp: Utc::now(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(opportunities)
    }

    fn clone_dyn(&self) -> Box<dyn ArbitrageStrategy> {
        Box::new(self.clone())
    }

    fn incremental_views(
        &self,
        updated: &std::collections::HashSet<common::types::TradingPair>,
    ) -> Vec<GraphView> {
        if let Some(target) = &self.config.target_dex {
            vec![GraphView::DexFiltered(target.clone())]
        } else if !updated.is_empty() {
            vec![self.required_graph_view()]
        } else {
            vec![]
        }
    }
}
