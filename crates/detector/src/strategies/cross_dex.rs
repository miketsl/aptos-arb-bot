use super::{ArbitrageStrategy, CrossDexConfig};
use crate::graph::PriceGraphView;
use anyhow::Result;
use async_trait::async_trait;
use common::types::{ArbitrageOpportunity, GraphView};
use rust_decimal::MathematicalOps;

#[derive(Clone)]
pub struct CrossDexArbitrage {
    _config: CrossDexConfig,
}

impl CrossDexArbitrage {
    pub fn new(config: CrossDexConfig) -> Self {
        Self { _config: config }
    }
}

#[async_trait]
impl ArbitrageStrategy for CrossDexArbitrage {
    fn name(&self) -> &str {
        "cross_dex_arbitrage"
    }

    fn required_graph_view(&self) -> GraphView {
        // This strategy needs to see all pools for a given pair to compare them.
        // The view creation logic will handle providing the right data.
        // For now, we can specify a generic view. The filtering will happen
        // during view creation based on what the strategy needs.
        GraphView::All
    }

    async fn detect_opportunities(
        &self,
        graph_view: &PriceGraphView,
        block_number: u64,
    ) -> Result<Vec<ArbitrageOpportunity>> {
        let mut opportunities = Vec::new();
        let mut processed_pairs = std::collections::HashSet::new();

        for (source_id, target_id, _) in graph_view.graph.all_edges() {
            let asset_x = &graph_view.asset_mapping[&source_id];
            let asset_y = &graph_view.asset_mapping[&target_id];

            let sorted_pair = if asset_x < asset_y {
                (asset_x.clone(), asset_y.clone())
            } else {
                (asset_y.clone(), asset_x.clone())
            };

            if processed_pairs.contains(&sorted_pair) {
                continue;
            }
            processed_pairs.insert(sorted_pair.clone());

            let forward_edges: Vec<_> = graph_view
                .graph
                .edges(source_id)
                .filter(|(_, target, _)| *target == target_id)
                .map(|(_, _, edge)| edge)
                .collect();

            let reverse_edges: Vec<_> = graph_view
                .graph
                .edges(target_id)
                .filter(|(_, target, _)| *target == source_id)
                .map(|(_, _, edge)| edge)
                .collect();

            for buy_edge in &forward_edges {
                for sell_edge in &reverse_edges {
                    if buy_edge.exchange == sell_edge.exchange {
                        continue;
                    }

                    if let (
                        super::super::graph::PoolModel::ConstantProduct {
                            reserve_x: reserve_x1,
                            reserve_y: reserve_y1,
                            ..
                        },
                        super::super::graph::PoolModel::ConstantProduct {
                            reserve_x: reserve_x2,
                            reserve_y: reserve_y2,
                            ..
                        },
                    ) = (&buy_edge.model, &sell_edge.model)
                    {
                        let price1 = reserve_y1.0 / reserve_x1.0;
                        let price2 = reserve_y2.0 / reserve_x2.0;

                        if price2 > price1 {
                            if let Some(sqrt_price) = (price1 * price2).sqrt() {
                                let optimal_input = sqrt_price * reserve_x1.0 - reserve_x1.0;
                                if optimal_input > rust_decimal::Decimal::ZERO {
                                    if let Some(amount_out) = buy_edge
                                        .quote(&common::types::Quantity(optimal_input), asset_x)
                                    {
                                        if let Some(final_amount) =
                                            sell_edge.quote(&amount_out, asset_y)
                                        {
                                            let profit = final_amount.0 - optimal_input;
                                            if profit > rust_decimal::Decimal::ZERO {
                                                let opportunity = ArbitrageOpportunity {
                                                    id: uuid::Uuid::new_v4(),
                                                    strategy: self.name().to_string(),
                                                    path: vec![
                                                        buy_edge.to_serializable(),
                                                        sell_edge.to_serializable(),
                                                    ],
                                                    expected_profit: profit,
                                                    input_amount: optimal_input,
                                                    gas_estimate: 0, // Placeholder
                                                    block_number,
                                                    timestamp: chrono::Utc::now(),
                                                };
                                                opportunities.push(opportunity);
                                            }
                                        }
                                    }
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
        // Only run cross-DEX on pairs that had updates in this block
        updated
            .iter()
            .cloned()
            .map(GraphView::PairFiltered)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, PoolModel, PriceGraph};
    use common::types::{GraphView, Quantity, TradingPair};
    use rust_decimal_macros::dec;
    use std::{str::FromStr, time::Instant};

    #[tokio::test]
    async fn test_cross_dex_detects_arbitrage() {
        let mut graph = PriceGraph::new();
        let asset_x = common::types::Asset::from_str("A").unwrap();
        let asset_y = common::types::Asset::from_str("B").unwrap();
        // Two pools with different reserves => different prices
        let edge1 = Edge {
            pair: TradingPair::new(asset_x.clone(), asset_y.clone()),
            exchange: "X".to_string(),
            pool_address: "p1".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(2)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        let edge2 = Edge {
            pair: TradingPair::new(asset_x.clone(), asset_y.clone()),
            exchange: "Y".to_string(),
            pool_address: "p2".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(3)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        graph.update_edge(edge1.clone());
        graph.update_edge(edge2.clone());
        // Also add reverse edges for the same pools
        // Add a reverse edge with favorable price for selling
        let rev = Edge {
            pair: TradingPair::new(asset_y.clone(), asset_x.clone()),
            exchange: "X".to_string(),
            pool_address: "p1_rev".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(10)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        graph.update_edge(rev);
        let view = graph.create_view(&GraphView::All);
        let strat = CrossDexArbitrage::new(CrossDexConfig::default());
        let opps = strat.detect_opportunities(&view, 0).await.unwrap();
        assert!(!opps.is_empty(), "expected cross-dex opportunity");
    }
}
