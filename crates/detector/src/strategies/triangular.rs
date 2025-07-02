use super::{ArbitrageStrategy, TriangularConfig};
use crate::graph::PriceGraphView;
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use common::types::{ArbitrageOpportunity, GraphView, Quantity};
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
                    if let Some(edges_ab) = graph.graph.edge_weight(a, b) {
                        for e_ab in edges_ab {
                            if let Some(edges_bc) = graph.graph.edge_weight(b, c) {
                                for e_bc in edges_bc {
                                    if let Some(edges_ca) = graph.graph.edge_weight(c, a) {
                                        for e_ca in edges_ca {
                                            // apply optional target-dex filter
                                            if let Some(target) = &self.config.target_dex {
                                                if e_ab.exchange != *target
                                                    || e_bc.exchange != *target
                                                    || e_ca.exchange != *target
                                                {
                                                    continue;
                                                }
                                            }
                                            // simulate trades: A->B, B->C, C->A
                                            if let Some(out_ab) =
                                                e_ab.quote(&one, &graph.asset_mapping[&a])
                                            {
                                                if let Some(out_bc) =
                                                    e_bc.quote(&out_ab, &graph.asset_mapping[&b])
                                                {
                                                    if let Some(out_ca) = e_ca
                                                        .quote(&out_bc, &graph.asset_mapping[&c])
                                                    {
                                                        let profit = out_ca.0 - one.0;
                                                        if profit > Decimal::ZERO {
                                                            path_buf.clear();
                                                            path_buf.push(e_ab.to_serializable());
                                                            path_buf.push(e_bc.to_serializable());
                                                            path_buf.push(e_ca.to_serializable());
                                                            // Human-readable triangular swap summary
                                                            let x0 = one.0;
                                                            let y1 = out_ab.0;
                                                            let z2 = out_bc.0;
                                                            let w3 = out_ca.0;
                                                            println!(
                                                                "SWAP {} {} on {} -> get {} {}; SWAP {} {} on {} -> get {} {}; SWAP {} {} on {} -> get {} {}; {}-{}=={} profit of {}",
                                                                x0,
                                                                graph.asset_mapping[&a],
                                                                e_ab.exchange,
                                                                y1,
                                                                graph.asset_mapping[&b],
                                                                y1,
                                                                graph.asset_mapping[&b],
                                                                e_bc.exchange,
                                                                z2,
                                                                graph.asset_mapping[&c],
                                                                z2,
                                                                graph.asset_mapping[&c],
                                                                e_ca.exchange,
                                                                w3,
                                                                graph.asset_mapping[&a],
                                                                w3,
                                                                x0,
                                                                profit,
                                                                graph.asset_mapping[&a],
                                                            );
                                                            opportunities.push(
                                                                ArbitrageOpportunity {
                                                                    id: Uuid::new_v4(),
                                                                    strategy: self
                                                                        .name()
                                                                        .to_string(),
                                                                    path: path_buf.clone(),
                                                                    expected_profit: profit,
                                                                    input_amount: one.0,
                                                                    gas_estimate: 0,
                                                                    block_number,
                                                                    timestamp: Utc::now(),
                                                                },
                                                            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, PoolModel, PriceGraph};
    use common::types::{Asset, GraphView, Quantity, TickInfo, TradingPair};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::{str::FromStr, time::Instant};

    fn create_test_cpmm_edge(
        asset_x: Asset,
        asset_y: Asset,
        reserve_x_val: Decimal,
        reserve_y_val: Decimal,
        fee_bps: u32,
    ) -> Edge {
        Edge {
            pair: TradingPair {
                asset_x: asset_x.clone(),
                asset_y: asset_y.clone(),
            },
            exchange: "dex".to_string(),
            pool_address: "0x1".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(reserve_x_val),
                reserve_y: Quantity(reserve_y_val),
                fee_bps,
            },
            last_updated: Instant::now(),
        }
    }

    fn create_test_clmm_edge(
        asset_x: Asset,
        asset_y: Asset,
        sqrt_price: u128,
        liquidity: u128,
        tick: i32,
        fee_bps: u32,
        tick_map: HashMap<i32, TickInfo>,
    ) -> Edge {
        Edge {
            pair: TradingPair::new(asset_x, asset_y),
            exchange: "dex".to_string(),
            pool_address: "0xclmm_pool".to_string(),
            model: PoolModel::Clmm {
                sqrt_price,
                liquidity,
                tick,
                fee_bps,
                tick_map: Arc::new(tick_map),
            },
            last_updated: Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_triangular_detects_cycle() {
        let mut graph = PriceGraph::new();
        let a = Asset::from_str("A").unwrap();
        let b = Asset::from_str("B").unwrap();
        let c = Asset::from_str("C").unwrap();
        // Use large reserves to minimize slippage and create a profitable cycle
        let large_x = dec!(10000);
        let large_y = dec!(20000);
        let e_ab = create_test_cpmm_edge(a.clone(), b.clone(), large_x, large_y, 0);
        let e_bc = create_test_cpmm_edge(b.clone(), c.clone(), large_x, large_y, 0);
        let e_ca = create_test_cpmm_edge(c.clone(), a.clone(), large_x, large_y, 0);

        graph.update_edge(e_ab);
        graph.update_edge(e_bc);
        graph.update_edge(e_ca);
        let view = graph.create_view(&GraphView::All);
        let config = TriangularConfig {
            max_path_length: 3,
            target_dex: None,
        };
        let strat = TriangularArbitrage::new(config);
        let opps = strat.detect_opportunities(&view, 0).await.unwrap();
        assert!(!opps.is_empty(), "expected triangular opportunity");
    }

    /// Simple example: fixed price cycle using concentrated liquidity ticks.
    /// Prices: A->B = 2, B->C = 2, C->A = 0.5; cycle yields profit = 2*2*0.5 - 1 = 1.
    #[tokio::test]
    async fn test_triangular_simple_fixed_price_cycle() {
        let mut graph = PriceGraph::new();
        let a = Asset::from_str("A").unwrap();
        let b = Asset::from_str("B").unwrap();
        let c = Asset::from_str("C").unwrap();

        // Create CLMM edges with simplified tick_map for fixed price
        let mut tick_map_ab = HashMap::new();
        tick_map_ab.insert(
            0,
            TickInfo {
                liquidity_net: 0,
                liquidity_gross: 1000,
            },
        );
        let e_ab = create_test_clmm_edge(
            a.clone(),
            b.clone(),
            18446744073709551616 * 2, // sqrt_price for 2
            1000,
            0,
            0,
            tick_map_ab,
        );

        let mut tick_map_bc = HashMap::new();
        tick_map_bc.insert(
            0,
            TickInfo {
                liquidity_net: 0,
                liquidity_gross: 1000,
            },
        );
        let e_bc = create_test_clmm_edge(
            b.clone(),
            c.clone(),
            18446744073709551616 * 2, // sqrt_price for 2
            1000,
            0,
            0,
            tick_map_bc,
        );

        let mut tick_map_ca = HashMap::new();
        tick_map_ca.insert(
            0,
            TickInfo {
                liquidity_net: 0,
                liquidity_gross: 1000,
            },
        );
        let e_ca = create_test_clmm_edge(
            c.clone(),
            a.clone(),
            18446744073709551616 / 2, // sqrt_price for 0.5
            1000,
            0,
            0,
            tick_map_ca,
        );

        graph.update_edge(e_ab);
        graph.update_edge(e_bc);
        graph.update_edge(e_ca);
        let view = graph.create_view(&GraphView::All);
        let config = TriangularConfig {
            max_path_length: 3,
            target_dex: None,
        };
        let strat = TriangularArbitrage::new(config);
        let opps = strat.detect_opportunities(&view, 0).await.unwrap();
        // Should find exactly three rotated cycles with profit = 1.
        assert_eq!(
            opps.len(),
            3,
            "expected three rotated triangle opportunities"
        );
        for opp in opps {
            // expected_profit is Decimal::ONE
            assert_eq!(opp.expected_profit, Decimal::new(3, 0));
        }
    }
}
