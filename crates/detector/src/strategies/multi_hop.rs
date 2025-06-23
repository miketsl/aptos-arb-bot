use super::{ArbitrageStrategy, MultiHopConfig};
use crate::graph::{AssetId, Edge, PriceGraphView};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use common::types::{ArbitrageOpportunity, GraphView, Quantity};
use rust_decimal::Decimal;
use std::collections::HashSet;
use uuid::Uuid;

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
        let mut opportunities = Vec::new();
        let one = Quantity(Decimal::ONE);
        let max_hops = self.config.max_hops;
        let enable_cross_dex = self.config.enable_cross_dex;

        let mut visited = HashSet::with_capacity(max_hops * 2);
        let mut path = Vec::with_capacity(max_hops);

        for start in graph.graph.nodes() {
            fn dfs<'a>(
                start: AssetId,
                current: AssetId,
                graph: &'a PriceGraphView,
                visited: &mut HashSet<AssetId>,
                path: &mut Vec<&'a Edge>,
                one: Quantity,
                max_hops: usize,
                enable_cross_dex: bool,
                opportunities: &mut Vec<ArbitrageOpportunity>,
                block_number: u64,
            ) {
                if path.len() > 0 && current == start {
                    let mut amount = one;
                    for edge in path.iter() {
                        if let Some(out) = edge.quote(&amount, &edge.pair.asset_x) {
                            amount = out;
                        } else {
                            return;
                        }
                    }
                    let profit = amount.0 - one.0;
                    if profit > Decimal::ZERO {
                        let path_serialized = path.iter().map(|e| e.to_serializable()).collect();
                        opportunities.push(ArbitrageOpportunity {
                            id: Uuid::new_v4(),
                            strategy: "multi_hop_arbitrage".to_string(),
                            path: path_serialized,
                            expected_profit: profit,
                            input_amount: one.0,
                            gas_estimate: 0,
                            block_number,
                            timestamp: Utc::now(),
                        });
                    }
                    return;
                }
                if path.len() >= max_hops {
                    return;
                }
                for neighbor in graph.graph.neighbors(current) {
                    if neighbor != start && visited.contains(&neighbor) {
                        continue;
                    }
                    if let Some(edges) = graph.graph.edge_weight(current, neighbor) {
                        for edge in edges {
                            if !enable_cross_dex {
                                if let Some(first_edge) = path.first() {
                                    if edge.exchange != first_edge.exchange {
                                        continue;
                                    }
                                }
                            }
                            visited.insert(neighbor);
                            path.push(edge);
                            dfs(
                                start,
                                neighbor,
                                graph,
                                visited,
                                path,
                                one,
                                max_hops,
                                enable_cross_dex,
                                opportunities,
                                block_number,
                            );
                            path.pop();
                            visited.remove(&neighbor);
                        }
                    }
                }
            }
            visited.clear();
            path.clear();
            visited.insert(start);
            dfs(
                start,
                start,
                graph,
                &mut visited,
                &mut path,
                one,
                max_hops,
                enable_cross_dex,
                &mut opportunities,
                block_number,
            );
        }
        Ok(opportunities)
    }

    fn clone_dyn(&self) -> Box<dyn ArbitrageStrategy> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, PoolModel, PriceGraph};
    use common::types::{Asset, GraphView, Quantity, TradingPair};
    use rust_decimal_macros::dec;
    use std::{str::FromStr, time::Instant};

    #[tokio::test]
    async fn test_multi_hop_detects_cycle() {
        let mut graph = PriceGraph::new();
        let a = Asset::from_str("A").unwrap();
        let b = Asset::from_str("B").unwrap();
        let c = Asset::from_str("C").unwrap();
        let large_x = dec!(10000);
        let large_y = dec!(20000);
        let e_ab = Edge {
            pair: TradingPair::new(a.clone(), b.clone()),
            exchange: "dex".to_string(),
            pool_address: "p_ab".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(large_x),
                reserve_y: Quantity(large_y),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        let e_bc = Edge {
            pair: TradingPair::new(b.clone(), c.clone()),
            exchange: "dex".to_string(),
            pool_address: "p_bc".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(large_x),
                reserve_y: Quantity(large_y),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        let e_ca = Edge {
            pair: TradingPair::new(c.clone(), a.clone()),
            exchange: "dex".to_string(),
            pool_address: "p_ca".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(large_x),
                reserve_y: Quantity(large_y),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        };
        graph.update_edge(e_ab);
        graph.update_edge(e_bc);
        graph.update_edge(e_ca);
        let view = graph.create_view(&GraphView::All);
        let config = MultiHopConfig {
            max_hops: 3,
            min_liquidity: dec!(0),
            enable_cross_dex: true,
        };
        let strat = MultiHopArbitrage::new(config);
        let opps = strat.detect_opportunities(&view, 0).await.unwrap();
        assert!(!opps.is_empty(), "expected multi-hop opportunity");
    }
}
