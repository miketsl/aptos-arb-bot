use crate::graph::{Edge, PoolModel};
use anyhow::Result;
use common::types::{Asset, MarketUpdate, TradingPair};
use std::{str::FromStr, sync::Arc, time::Instant};

/// Transforms a market update into a graph edge.
pub fn transform_update(update: MarketUpdate) -> Result<Edge> {
    let (pool_address, dex_name, token_pair, model) = match update {
        MarketUpdate::Clmm(data) => {
            let asset_x = Asset::from_str(&data.token_pair.token0)?;
            let asset_y = Asset::from_str(&data.token_pair.token1)?;
            (
                data.pool_address,
                data.dex_name,
                TradingPair::new(asset_x, asset_y),
                PoolModel::Clmm {
                    sqrt_price: data.sqrt_price,
                    liquidity: data.liquidity,
                    tick: data.tick,
                    fee_bps: data.fee_bps,
                    tick_map: Arc::new(data.tick_map),
                },
            )
        }
        MarketUpdate::ConstantProduct(data) => {
            let asset_x = Asset::from_str(&data.token_pair.token0)?;
            let asset_y = Asset::from_str(&data.token_pair.token1)?;
            (
                data.pool_address,
                data.dex_name,
                TradingPair::new(asset_x, asset_y),
                PoolModel::ConstantProduct {
                    reserve_x: data.reserve_x,
                    reserve_y: data.reserve_y,
                    fee_bps: data.fee_bps,
                },
            )
        }
        MarketUpdate::StableSwap(data) => {
            let asset_x = Asset::from_str(&data.token_pair.token0)?;
            let asset_y = Asset::from_str(&data.token_pair.token1)?;
            (
                data.pool_address,
                data.dex_name,
                TradingPair::new(asset_x, asset_y),
                PoolModel::StableSwap {
                    reserves: data.reserves,
                    amplification_factor: data.amplification_factor,
                    fee_bps: data.fee_bps,
                },
            )
        }
        MarketUpdate::WeightedPool(data) => {
            let asset_x = Asset::from_str(&data.token_pair.token0)?;
            let asset_y = Asset::from_str(&data.token_pair.token1)?;
            (
                data.pool_address,
                data.dex_name,
                TradingPair::new(asset_x, asset_y),
                PoolModel::WeightedPool {
                    reserves: data.reserves,
                    weights: data.weights,
                    fee_bps: data.fee_bps,
                },
            )
        }
    };

    Ok(Edge {
        pair: token_pair,
        exchange: dex_name,
        pool_address,
        model,
        last_updated: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{
        ClmmMarketUpdate, ConstantProductMarketUpdate, Quantity, StableSwapMarketUpdate, TickInfo,
        WeightedPoolMarketUpdate,
    };
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    #[test]
    fn test_transform_clmm() {
        let update = ClmmMarketUpdate {
            pool_address: "0xCLMM_POOL".to_string(),
            dex_name: "Hyperion".to_string(),
            token_pair: common::types::TokenPair {
                token0: "0xA".to_string(),
                token1: "0xB".to_string(),
            },
            sqrt_price: 1u128 << 64,
            liquidity: 1_000_000u128,
            tick: 100,
            fee_bps: 30,
            tick_map: HashMap::from([
                (
                    0,
                    TickInfo {
                        liquidity_net: 100,
                        liquidity_gross: 100,
                    },
                ),
                (
                    100,
                    TickInfo {
                        liquidity_net: 200,
                        liquidity_gross: 200,
                    },
                ),
            ]),
        };
        let edge = transform_update(MarketUpdate::Clmm(update)).expect("transform failed");
        match edge.model {
            PoolModel::Clmm {
                sqrt_price,
                liquidity,
                tick,
                fee_bps,
                tick_map,
            } => {
                assert_eq!(sqrt_price, 1u128 << 64);
                assert_eq!(liquidity, 1_000_000u128);
                assert_eq!(tick, 100);
                assert_eq!(fee_bps, 30);
                assert_eq!(tick_map.len(), 2);
            }
            _ => panic!("expected Clmm model"),
        }
        assert_eq!(edge.pool_address, "0xCLMM_POOL");
        assert_eq!(edge.exchange, "Hyperion");
        assert_eq!(edge.pair.asset_x, Asset::from_str("0xA").unwrap());
        assert_eq!(edge.pair.asset_y, Asset::from_str("0xB").unwrap());
    }

    #[test]
    fn test_transform_constant_product() {
        let update = ConstantProductMarketUpdate {
            pool_address: "0xCP_POOL".to_string(),
            dex_name: "Tapp".to_string(),
            token_pair: common::types::TokenPair {
                token0: "0xC".to_string(),
                token1: "0xD".to_string(),
            },
            reserve_x: Quantity(dec!(1000)),
            reserve_y: Quantity(dec!(100)),
            fee_bps: 25,
        };
        let edge =
            transform_update(MarketUpdate::ConstantProduct(update)).expect("transform failed");
        match edge.model {
            PoolModel::ConstantProduct {
                reserve_x,
                reserve_y,
                fee_bps,
            } => {
                assert_eq!(reserve_x, Quantity(dec!(1000)));
                assert_eq!(reserve_y, Quantity(dec!(100)));
                assert_eq!(fee_bps, 25);
            }
            _ => panic!("expected ConstantProduct model"),
        }
        assert_eq!(edge.pool_address, "0xCP_POOL");
        assert_eq!(edge.exchange, "Tapp");
    }

    #[test]
    fn test_transform_stable_swap() {
        let update = StableSwapMarketUpdate {
            pool_address: "0xSS_POOL".to_string(),
            dex_name: "Tapp".to_string(),
            token_pair: common::types::TokenPair {
                token0: "0xE".to_string(),
                token1: "0xF".to_string(),
            },
            reserves: vec![Quantity(dec!(1_000_000)), Quantity(dec!(1_000_000))],
            amplification_factor: 1000,
            fee_bps: 4,
        };
        let edge = transform_update(MarketUpdate::StableSwap(update)).expect("transform failed");
        match edge.model {
            PoolModel::StableSwap {
                reserves,
                amplification_factor,
                fee_bps,
            } => {
                assert_eq!(reserves.len(), 2);
                assert_eq!(reserves[0], Quantity(dec!(1_000_000)));
                assert_eq!(amplification_factor, 1000);
                assert_eq!(fee_bps, 4);
            }
            _ => panic!("expected StableSwap model"),
        }
        assert_eq!(edge.pool_address, "0xSS_POOL");
        assert_eq!(edge.exchange, "Tapp");
    }

    #[test]
    fn test_transform_weighted_pool() {
        let update = WeightedPoolMarketUpdate {
            pool_address: "0xWP_POOL".to_string(),
            dex_name: "Thala".to_string(),
            token_pair: common::types::TokenPair {
                token0: "0xG".to_string(),
                token1: "0xH".to_string(),
            },
            reserves: vec![Quantity(dec!(500)), Quantity(dec!(1000))],
            weights: vec![500000, 500000],
            fee_bps: 10,
        };
        let edge = transform_update(MarketUpdate::WeightedPool(update)).expect("transform failed");
        match edge.model {
            PoolModel::WeightedPool {
                reserves,
                weights,
                fee_bps,
            } => {
                assert_eq!(reserves.len(), 2);
                assert_eq!(reserves[0], Quantity(dec!(500)));
                assert_eq!(weights.len(), 2);
                assert_eq!(weights[0], 500000);
                assert_eq!(fee_bps, 10);
            }
            _ => panic!("expected WeightedPool model"),
        }
        assert_eq!(edge.pool_address, "0xWP_POOL");
        assert_eq!(edge.exchange, "Thala");
    }
}
