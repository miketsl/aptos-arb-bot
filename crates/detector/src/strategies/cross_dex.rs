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
}
