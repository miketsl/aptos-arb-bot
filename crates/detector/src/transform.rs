use crate::{
    exchange_const::Exchange,
    graph::{Edge, PoolModel, Tick},
};
use anyhow::{anyhow, Result};
use common::types::{Asset, MarketUpdate, Quantity, TradingPair};
use rust_decimal::Decimal;
use std::{str::FromStr, time::Instant};

/// Transforms a market update into a graph edge.
pub fn transform_update(update: MarketUpdate) -> Result<Edge> {
    let asset_x = Asset::from_str(&update.token_pair.token0)?;
    let asset_y = Asset::from_str(&update.token_pair.token1)?;

    // This is a simplified assumption. In a real scenario, we might have a field
    // in MarketUpdate to distinguish between pool types.
    let model = if update.tick_map.is_empty() {
        // Assume ConstantProduct if tick_map is empty
        let (reserve_x, reserve_y) = reserves_from_liquidity_and_sqrt_price(
            update.liquidity,
            update.sqrt_price,
            // These decimals need to be part of the asset definition
            6, // decimals_x
            6, // decimals_y
        )?;
        PoolModel::ConstantProduct {
            reserve_x,
            reserve_y,
            fee_bps: update.fee_bps as u16,
        }
    } else {
        // Assume ConcentratedLiquidity if tick_map is not empty
        let ticks = update
            .tick_map
            .into_iter()
            .map(|(price, info)| {
                Ok(Tick {
                    price: Decimal::from(price),
                    liquidity_gross: Decimal::from(info.liquidity_gross),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        PoolModel::ConcentratedLiquidity {
            ticks,
            fee_bps: update.fee_bps as u16,
        }
    };

    Ok(Edge {
        pair: TradingPair::new(asset_x, asset_y),
        exchange: Exchange::from_str(&update.dex_name)
            .map_err(|_| anyhow!("Unknown exchange: {}", update.dex_name))?,
        pool_address: update.pool_address,
        model,
        last_updated: Instant::now(),
    })
}

/// Calculates token reserves from liquidity and sqrt_price for a CPMM pool.
/// This is a simplified calculation and may need adjustment based on the specific
/// DEX's formulas.
fn reserves_from_liquidity_and_sqrt_price(
    liquidity: u128,
    sqrt_price_q64: u128,
    decimals_x: u32,
    decimals_y: u32,
) -> Result<(Quantity, Quantity)> {
    let liquidity = Decimal::from(liquidity);
    let sqrt_price = Decimal::from(sqrt_price_q64) / Decimal::from(2u64.pow(64));

    // reserve_y = liquidity / sqrt_price
    let reserve_y_unscaled = liquidity / sqrt_price;
    // reserve_x = liquidity * sqrt_price
    let reserve_x_unscaled = liquidity * sqrt_price;

    let reserve_x = Quantity(reserve_x_unscaled / Decimal::from(10u64.pow(decimals_x)));
    let reserve_y = Quantity(reserve_y_unscaled / Decimal::from(10u64.pow(decimals_y)));

    Ok((reserve_x, reserve_y))
}
