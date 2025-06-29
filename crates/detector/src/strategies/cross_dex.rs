use super::{ArbitrageStrategy, CrossDexConfig};
use crate::graph::{Edge, PriceGraphView};
use anyhow::Result;
use async_trait::async_trait;
use common::types::TradingPair;
use common::types::{ArbitrageOpportunity, GraphView};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

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
        // For now, we can specify a generic view.
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
            processed_pairs.insert(sorted_pair);

            let forward_edges: Vec<&Edge> = graph_view
                .graph
                .edges(source_id)
                .filter(|(_, target, _)| *target == target_id)
                .flat_map(|(_, _, edges)| edges.iter())
                .collect();

            let reverse_edges: Vec<&Edge> = graph_view
                .graph
                .edges(target_id)
                .filter(|(_, target, _)| *target == source_id)
                .flat_map(|(_, _, edges)| edges.iter())
                .collect();

            for &buy_edge in &forward_edges {
                for &sell_edge in &reverse_edges {
                    if buy_edge.exchange == sell_edge.exchange {
                        continue;
                    }

                    // Compute optimal cross-DEX input by trying a few input amounts
                    // This is a simplified approach for multi-pool type arbitrage.
                    // A more sophisticated approach would involve numerical optimization.
                    let test_input_amounts = vec![
                        Decimal::from_f64(0.1).unwrap(),
                        Decimal::from_f64(1.0).unwrap(),
                        Decimal::from_f64(10.0).unwrap(),
                        Decimal::from_f64(100.0).unwrap(),
                        Decimal::from_f64(1000.0).unwrap(),
                    ];

                    for &optimal_in_val in &test_input_amounts {
                        let optimal_in = common::types::Quantity(optimal_in_val);

                        if let Some(mid) = buy_edge.quote(&optimal_in, asset_x) {
                            if let Some(out) = sell_edge.quote(&mid, asset_y) {
                                let profit = out.0 - optimal_in.0;
                                if profit > Decimal::ZERO {
                                    // Human-readable arbitrage summary
                                    println!(
                                        "SWAP {} {} on {} -> get {} {}; SWAP {} {} on {} -> get {} {}; {}-{}=={} profit of {}",
                                        optimal_in.0,
                                        asset_x,
                                        buy_edge.exchange,
                                        mid.0,
                                        asset_y,
                                        mid.0,
                                        asset_y,
                                        sell_edge.exchange,
                                        out.0,
                                        asset_x,
                                        out.0,
                                        optimal_in.0,
                                        profit,
                                        asset_x,
                                    );
                                    opportunities.push(ArbitrageOpportunity {
                                        id: uuid::Uuid::new_v4(),
                                        strategy: self.name().to_string(),
                                        path: vec![
                                            buy_edge.to_serializable(),
                                            sell_edge.to_serializable(),
                                        ],
                                        expected_profit: profit,
                                        input_amount: optimal_in.0,
                                        gas_estimate: 0,
                                        block_number,
                                        timestamp: chrono::Utc::now(),
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
        // Only run cross-DEX once per unordered pair that had updates in this block
        let mut seen = std::collections::HashSet::new();
        updated
            .iter()
            .filter_map(|tp| {
                let sorted = if tp.asset_x < tp.asset_y {
                    (tp.asset_x.clone(), tp.asset_y.clone())
                } else {
                    (tp.asset_y.clone(), tp.asset_x.clone())
                };
                if seen.insert(sorted.clone()) {
                    Some(GraphView::PairFiltered(TradingPair::new(
                        sorted.0, sorted.1,
                    )))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::graph::{Edge, PoolModel, PriceGraph};
    use common::types::{Asset, GraphView, Quantity, TradingPair};
    use rust_decimal_macros::dec;
    use std::time::Instant;

    #[tokio::test]
    async fn test_cross_dex_arbitrage_logic() {
        let mut graph = PriceGraph::new();
        let a = Asset::from("A");
        let b = Asset::from("B");
        // D1: A->B = 1.5, B->A = 0.666...
        let e1 = Edge {
            pair: TradingPair::new(a.clone(), b.clone()),
            exchange: "D1".to_string(),
            pool_address: "p1".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(1.5)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        let e2 = Edge {
            pair: TradingPair::new(b.clone(), a.clone()),
            exchange: "D1".to_string(),
            pool_address: "p1_rev".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(0.6666667)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        // D2: A->B = 2.0, B->A = 0.5
        let e3 = Edge {
            pair: TradingPair::new(a.clone(), b.clone()),
            exchange: "D2".to_string(),
            pool_address: "p2".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(2)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        let e4 = Edge {
            pair: TradingPair::new(b.clone(), a.clone()),
            exchange: "D2".to_string(),
            pool_address: "p2_rev".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(0.5)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        graph.update_edge(e1);
        graph.update_edge(e2);
        graph.update_edge(e3);
        graph.update_edge(e4);
        let view = graph.create_view(&GraphView::All);
        let strat = CrossDexArbitrage::new(CrossDexConfig {});
        let opps = strat.detect_opportunities(&view, 0).await.unwrap();
        assert!(!opps.is_empty(), "Cross-DEX arbitrage was not detected");
    }
}
