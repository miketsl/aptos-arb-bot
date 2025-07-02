use common::types::TickInfo;
use common::types::{Asset, Quantity, TradingPair};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub mod pool_math;
pub mod state;

pub use state::{PriceGraph, PriceGraphView};

/// A copyable asset identifier for use as DiGraphMap node
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(u64);

impl AssetId {
    pub fn new(id: u64) -> Self {
        AssetId(id)
    }
}

/// Represents the model of a liquidity pool.
#[derive(Debug, Clone, PartialEq)]
pub enum PoolModel {
    ConstantProduct {
        reserve_x: Quantity,
        reserve_y: Quantity,
        fee_bps: u32,
    },
    Clmm {
        sqrt_price: u128,
        liquidity: u128,
        tick: i32,
        fee_bps: u32,
        tick_map: Arc<HashMap<i32, TickInfo>>,
    },
    StableSwap {
        reserves: Vec<Quantity>,
        amplification_factor: u128,
        fee_bps: u32,
    },
    WeightedPool {
        reserves: Vec<Quantity>,
        weights: Vec<u32>,
        fee_bps: u32,
    },
}

impl PoolModel {
    pub fn fee_bps(&self) -> u32 {
        match self {
            PoolModel::ConstantProduct { fee_bps, .. } => *fee_bps,
            PoolModel::Clmm { fee_bps, .. } => *fee_bps,
            PoolModel::StableSwap { fee_bps, .. } => *fee_bps,
            PoolModel::WeightedPool { fee_bps, .. } => *fee_bps,
        }
    }
}

/// Represents an edge in the price graph, corresponding to a specific swap direction in a pool.
/// The Edge itself will be the weight in the DiGraphMap.
/// To ensure PartialEq and Eq for DiGraphMap, we might need to be careful if Edge contains non-Eq fields like Instant directly.
/// However, petgraph::graphmap::DiGraphMap uses the node type's Eq and Hash. Edge is a weight.
#[derive(Debug, Clone)]
pub struct Edge {
    pub pair: TradingPair, // Defines direction: asset_x -> asset_y
    pub exchange: String,
    pub pool_address: String,
    pub model: PoolModel,
    pub last_updated: Instant,
}

// Manual PartialEq implementation for Edge if needed due to Instant, though for graph weights it's not strictly necessary for map keys.
// For now, relying on derive(Clone) and using it as a weight.
// If Edge were a node, it would need Eq and Hash.
impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        // Equality should be deterministic and must **not** depend on wall-clock
        // time. `last_updated` is therefore intentionally *excluded*.
        self.pair == other.pair && self.exchange == other.exchange && self.model == other.model
    }
}

impl Edge {
    pub fn to_serializable(&self) -> common::types::SerializableEdge {
        common::types::SerializableEdge {
            pair: self.pair.clone(),
            exchange: self.exchange.clone(),
            pool_address: self.pool_address.clone(),
            liquidity: Default::default(), // Placeholder
            fee_bps: self.model.fee_bps(),
            last_updated: chrono::Utc::now(), // Placeholder
            last_opportunity: None,
            opportunity_count: 0,
            total_volume: Default::default(),
        }
    }

    /// Calculates the output amount for a given input amount based on the pool model.
    pub fn quote(&self, amount_in: &Quantity, asset_in: &Asset) -> Option<Quantity> {
        if *asset_in != self.pair.asset_x {
            return None;
        }

        let fee_decimal =
            Decimal::from_u32(self.model.fee_bps()).unwrap_or_default() / Decimal::new(10000, 0);
        let amount_in_after_fee = amount_in.0 * (Decimal::ONE - fee_decimal);

        if amount_in_after_fee <= Decimal::ZERO {
            return None;
        }

        match &self.model {
            PoolModel::ConstantProduct {
                reserve_x,
                reserve_y,
                .. // fee_bps is handled above
            } => {
                if reserve_x.0.is_zero() || reserve_y.0.is_zero() {
                    return None;
                }

                let amount_out_val =
                    (reserve_y.0 * amount_in_after_fee) / (reserve_x.0 + amount_in_after_fee);

                if amount_out_val.is_sign_negative()
                    || amount_out_val.is_zero()
                    || amount_out_val > reserve_y.0
                {
                    return None; // Not enough liquidity or invalid amount
                }
                Some(Quantity(amount_out_val))
            }
            PoolModel::Clmm {
                sqrt_price,
                liquidity,
                tick,
                tick_map,
                .. // fee_bps is handled above
            } => pool_math::quote_clmm(
                amount_in_after_fee,
                *sqrt_price,
                *liquidity,
                *tick,
                tick_map.clone(),
            ),
            PoolModel::StableSwap {
                reserves,
                amplification_factor,
                .. // fee_bps is handled above
            } => pool_math::quote_stableswap(amount_in_after_fee, reserves, *amplification_factor),
            PoolModel::WeightedPool {
                reserves,
                weights,
                .. // fee_bps is handled above
            } => pool_math::quote_weighted(amount_in_after_fee, reserves, weights),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{Asset, Quantity, TickInfo, TradingPair};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use std::str::FromStr;

    fn usdc_asset() -> Asset {
        Asset::from_str("0x1::coin::USDC").unwrap()
    }
    fn apt_asset() -> Asset {
        Asset::from_str("0x1::aptos_coin::AptosCoin").unwrap()
    }
    fn eth_asset() -> Asset {
        Asset::from_str("0x1::coin::ETH").unwrap()
    }

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
            exchange: "tapp".to_string(), // Placeholder
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
            exchange: "hyperion".to_string(),
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

    fn create_test_stableswap_edge(
        asset_x: Asset,
        asset_y: Asset,
        reserves: Vec<Quantity>,
        amplification_factor: u128,
        fee_bps: u32,
    ) -> Edge {
        Edge {
            pair: TradingPair::new(asset_x, asset_y),
            exchange: "tapp".to_string(),
            pool_address: "0xstableswap_pool".to_string(),
            model: PoolModel::StableSwap {
                reserves,
                amplification_factor,
                fee_bps,
            },
            last_updated: Instant::now(),
        }
    }

    fn create_test_weighted_edge(
        asset_x: Asset,
        asset_y: Asset,
        reserves: Vec<Quantity>,
        weights: Vec<u32>,
        fee_bps: u32,
    ) -> Edge {
        Edge {
            pair: TradingPair::new(asset_x, asset_y),
            exchange: "thala".to_string(),
            pool_address: "0xweighted_pool".to_string(),
            model: PoolModel::WeightedPool {
                reserves,
                weights,
                fee_bps,
            },
            last_updated: Instant::now(),
        }
    }

    #[test]
    fn test_update_edge_and_view() {
        let mut graph = PriceGraph::new();
        let usdc = usdc_asset();
        let apt = apt_asset();
        let edge1 = create_test_cpmm_edge(usdc.clone(), apt.clone(), dec!(1000), dec!(100), 30);

        graph.update_edge(edge1.clone());

        let view = graph.create_view(&common::types::GraphView::All);
        assert_eq!(view.graph.edge_count(), 1);
        assert_eq!(view.graph.node_count(), 2);

        let source_id = view
            .asset_mapping
            .iter()
            .find(|(_, asset)| **asset == usdc)
            .map(|(id, _)| *id)
            .unwrap();
        let target_id = view
            .asset_mapping
            .iter()
            .find(|(_, asset)| **asset == apt)
            .map(|(id, _)| *id)
            .unwrap();

        assert!(view.graph.contains_edge(source_id, target_id));
    }

    #[test]
    fn test_prune_stale() {
        let mut graph = PriceGraph::new();
        let usdc = usdc_asset();
        let apt = apt_asset();
        let eth = eth_asset();

        let fresh_edge =
            create_test_cpmm_edge(usdc.clone(), apt.clone(), dec!(1000), dec!(100), 30);
        let mut stale_edge = create_test_cpmm_edge(apt.clone(), eth.clone(), dec!(50), dec!(1), 30);
        stale_edge.last_updated = Instant::now() - std::time::Duration::from_secs(10);

        graph.update_edge(fresh_edge.clone());
        graph.update_edge(stale_edge.clone());

        let view1 = graph.create_view(&common::types::GraphView::All);
        assert_eq!(view1.graph.edge_count(), 2);

        graph.prune_stale(std::time::Duration::from_secs(5));

        let view2 = graph.create_view(&common::types::GraphView::All);
        assert_eq!(view2.graph.edge_count(), 1);

        let source_id = view2
            .asset_mapping
            .iter()
            .find(|(_, asset)| **asset == usdc)
            .map(|(id, _)| *id)
            .unwrap();
        let target_id = view2
            .asset_mapping
            .iter()
            .find(|(_, asset)| **asset == apt)
            .map(|(id, _)| *id)
            .unwrap();

        assert!(view2.graph.contains_edge(source_id, target_id));
    }

    #[test]
    fn test_edge_quote_simple_cpmm() {
        // USDC -> APT pool
        let edge = create_test_cpmm_edge(usdc_asset(), apt_asset(), dec!(10000), dec!(1000), 25); // 0.25% fee

        // Quote selling 100 USDC for APT
        let amount_in_usdc = Quantity(dec!(100));
        let quote_result = edge.quote(&amount_in_usdc, &usdc_asset());

        assert!(quote_result.is_some());
        let amount_out_apt = quote_result.unwrap();

        // Manual calculation:
        // fee = 100 * (25/10000) = 0.25 USDC
        // amount_in_after_fee = 100 - 0.25 = 99.75 USDC
        // reserve_x = 10000, reserve_y = 1000
        // amount_out = reserve_y * amount_in_after_fee / (reserve_x + amount_in_after_fee)
        // amount_out = 1000 * 99.75 / (10000 + 99.75)
        // amount_out = 99750 / 10099.75 = 9.876479...
        let expected_out = dec!(1000) * (dec!(100) * (Decimal::ONE - dec!(0.0025)))
            / (dec!(10000) + (dec!(100) * (Decimal::ONE - dec!(0.0025))));
        assert_eq!(amount_out_apt.0.round_dp(8), expected_out.round_dp(8));

        // Quote selling asset not in pair (APT for APT)
        let amount_in_apt_wrong = Quantity(dec!(10));
        assert!(
            edge.quote(&amount_in_apt_wrong, &apt_asset()).is_none(),
            "Quoting with wrong asset should fail"
        );

        // Quote selling asset that is asset_y (APT for USDC, but edge is USDC -> APT)
        let amount_in_apt_reverse = Quantity(dec!(10));
        assert!(
            edge.quote(&amount_in_apt_reverse, &apt_asset()).is_none(),
            "Quoting with reverse asset should fail"
        );
    }

    #[test]
    fn test_edge_quote_insufficient_liquidity() {
        let edge = create_test_cpmm_edge(usdc_asset(), apt_asset(), dec!(100), dec!(10), 25);

        // Try to swap more than available in reserve_x after fee
        let large_amount_in = Quantity(dec!(10000));
        let quote_result = edge.quote(&large_amount_in, &usdc_asset());
        // The formula itself might not show this as an error directly unless output is > reserve_y
        // Let's test if output is greater than reserve_y
        // amount_in_after_fee = 10000 * (1 - 0.0025) = 9975
        // amount_out = 10 * 9975 / (100 + 9975) = 99750 / 10075 = 9.9007...
        // This is valid and < 10.
        assert!(quote_result.is_some());
        assert!(quote_result.unwrap().0 <= dec!(10));

        // Test with zero reserve
        let zero_reserve_edge =
            create_test_cpmm_edge(usdc_asset(), apt_asset(), dec!(0), dec!(10), 25);
        let amount_in = Quantity(dec!(10));
        assert!(zero_reserve_edge.quote(&amount_in, &usdc_asset()).is_none());

        let zero_reserve_edge_y =
            create_test_cpmm_edge(usdc_asset(), apt_asset(), dec!(10), dec!(0), 25);
        assert!(zero_reserve_edge_y
            .quote(&amount_in, &usdc_asset())
            .is_none());
    }

    #[test]
    fn test_clmm_quote() {
        let mut tick_map = HashMap::new();
        tick_map.insert(
            0,
            TickInfo {
                liquidity_net: 100,
                liquidity_gross: 100,
            },
        );
        let edge = create_test_clmm_edge(
            usdc_asset(),
            apt_asset(),
            2u128.pow(64), // sqrt_price for 1:1
            1_000_000,
            0,
            30,
            tick_map,
        );
        let amount_in = Quantity(dec!(100));
        let quote_result = edge.quote(&amount_in, &usdc_asset());
        assert!(quote_result.is_some());
        // Note: The CLMM math is still simplified, this test will pass with the current implementation.
        // A more robust test would require a more complex tick setup.
        let amount_out = quote_result.unwrap().0;
        assert!(amount_out > dec!(99) && amount_out < dec!(100));
    }

    #[test]
    fn test_stableswap_quote() {
        let edge = create_test_stableswap_edge(
            usdc_asset(),
            apt_asset(),
            vec![Quantity(dec!(1_000_000)), Quantity(dec!(1_000_000))],
            100,
            4,
        );
        let amount_in = Quantity(dec!(100));
        let quote_result = edge.quote(&amount_in, &usdc_asset());
        assert!(quote_result.is_some());
        // With 1:1 reserves, output should be close to input minus fee
        let amount_out = quote_result.unwrap().0;
        assert!(amount_out > dec!(99) && amount_out < dec!(100));
    }

    #[test]
    fn test_weighted_pool_quote() {
        let edge = create_test_weighted_edge(
            usdc_asset(),
            apt_asset(),
            vec![Quantity(dec!(1_000_000)), Quantity(dec!(1_000_000))],
            vec![500000, 500000], // 50/50 weight
            10,                   // 0.1% fee
        );
        let amount_in = Quantity(dec!(100));
        let quote_result = edge.quote(&amount_in, &usdc_asset());
        assert!(quote_result.is_some());
        let amount_out = quote_result.unwrap().0;
        // Calculation:
        // amount_in_after_fee = 100 * (1 - 0.001) = 99.9
        // r_in = 1_000_000, r_out = 1_000_000, w_in = 0.5, w_out = 0.5
        // ratio = 1_000_000 / (1_000_000 + 99.9) = 0.9999001...
        // power = 1
        // factor = 1 - ratio = 0.0000999...
        // amount_out = 1_000_000 * factor = 99.9...
        assert!(amount_out > dec!(99) && amount_out < dec!(100));
    }
}
