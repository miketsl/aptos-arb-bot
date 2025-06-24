use crate::graph::{Edge, PoolModel, Tick};
use anyhow::Result;
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
        exchange: update.dex_name,
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
    // Bounds checking to prevent overflow/underflow
    if sqrt_price_q64 == 0 {
        return Err(anyhow::anyhow!("sqrt_price cannot be zero"));
    }
    
    // Check for values that are too large for Decimal before conversion
    // Decimal::MAX is approximately 79,228,162,514,264,337,593,543,950,335
    const MAX_SAFE_U128: u128 = 79_228_162_514_264_337_593_543_950_335;
    
    if liquidity > MAX_SAFE_U128 {
        return Err(anyhow::anyhow!("liquidity value {} too large for decimal conversion", liquidity));
    }
    
    if sqrt_price_q64 > MAX_SAFE_U128 {
        return Err(anyhow::anyhow!("sqrt_price value {} too large for decimal conversion", sqrt_price_q64));
    }
    
    // Convert to Decimal with bounds checking
    let liquidity = Decimal::from(liquidity);
    
    // Interpret sqrt_price_q64 as Q64 fixed-point (divide by 2^64)
    let sqrt_price_raw = Decimal::from(sqrt_price_q64);
    
    let q64_divisor = Decimal::from(2u128.pow(64));
    let sqrt_price = sqrt_price_raw / q64_divisor;
    
    // Sanity check: sqrt_price should be positive and reasonable
    if sqrt_price <= Decimal::ZERO {
        return Err(anyhow::anyhow!("sqrt_price must be positive"));
    }
    
    // Additional bounds checking to prevent extreme calculations
    const MAX_REASONABLE_SQRT_PRICE: &str = "1000000000"; // 1 billion
    const MIN_REASONABLE_SQRT_PRICE: &str = "0.000000001"; // 1 nano
    
    let max_sqrt_price = Decimal::from_str(MAX_REASONABLE_SQRT_PRICE)?;
    let min_sqrt_price = Decimal::from_str(MIN_REASONABLE_SQRT_PRICE)?;
    
    if sqrt_price > max_sqrt_price || sqrt_price < min_sqrt_price {
        return Err(anyhow::anyhow!(
            "sqrt_price {} is outside reasonable bounds [{}, {}]",
            sqrt_price, min_sqrt_price, max_sqrt_price
        ));
    }

    // For CPMM pools: sqrt_price = sqrt(reserve_y / reserve_x)
    // Hence reserve_x = liquidity / sqrt_price, reserve_y = liquidity * sqrt_price
    let reserve_x_unscaled = match liquidity.checked_div(sqrt_price) {
        Some(result) => result,
        None => return Err(anyhow::anyhow!("Division overflow in reserve_x calculation")),
    };
    
    let reserve_y_unscaled = match liquidity.checked_mul(sqrt_price) {
        Some(result) => result,
        None => return Err(anyhow::anyhow!("Multiplication overflow in reserve_y calculation")),
    };

    let decimals_x_divisor = Decimal::from(10u64.pow(decimals_x));
    let decimals_y_divisor = Decimal::from(10u64.pow(decimals_y));
    
    let reserve_x = match reserve_x_unscaled.checked_div(decimals_x_divisor) {
        Some(result) => Quantity(result),
        None => return Err(anyhow::anyhow!("Division overflow in reserve_x scaling")),
    };
    
    let reserve_y = match reserve_y_unscaled.checked_div(decimals_y_divisor) {
        Some(result) => Quantity(result),
        None => return Err(anyhow::anyhow!("Division overflow in reserve_y scaling")),
    };

    Ok((reserve_x, reserve_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{MarketUpdate, Quantity, TickInfo, TokenPair};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use std::str::FromStr;

    fn basic_update() -> MarketUpdate {
        MarketUpdate {
            pool_address: "0xPOOL".to_string(),
            dex_name: "DEX".to_string(),
            token_pair: TokenPair {
                token0: "0xA".to_string(),
                token1: "0xB".to_string(),
            },
            sqrt_price: 1u128 << 64,
            liquidity: 1_000_000u128,
            tick: 0,
            fee_bps: 30,
            tick_map: HashMap::new(),
        }
    }

    #[test]
    fn test_transform_constant_product() {
        let update = basic_update();
        let edge = transform_update(update).expect("transform failed");
        // ConstantProduct branch: reserves should equal 1 after scaling
        match edge.model {
            PoolModel::ConstantProduct {
                reserve_x,
                reserve_y,
                fee_bps,
            } => {
                assert_eq!(reserve_x, Quantity(dec!(1)));
                assert_eq!(reserve_y, Quantity(dec!(1)));
                assert_eq!(fee_bps, 30);
            }
            _ => panic!("expected ConstantProduct model"),
        }
        assert_eq!(edge.pool_address, "0xPOOL");
        assert_eq!(edge.exchange, "DEX");
        // TradingPair direction preserved
        assert_eq!(edge.pair.asset_x, Asset::from_str("0xA").unwrap());
        assert_eq!(edge.pair.asset_y, Asset::from_str("0xB").unwrap());
    }

    #[test]
    fn test_transform_concentrated_liquidity() {
        let mut update = basic_update();
        // Add one tick entry to trigger CLMM branch
        let mut ticks = HashMap::new();
        ticks.insert(
            2,
            TickInfo {
                liquidity_net: 0,
                liquidity_gross: 5u128,
            },
        );
        update.tick_map = ticks;
        let edge = transform_update(update).expect("transform failed");
        match edge.model {
            PoolModel::ConcentratedLiquidity { ticks, fee_bps } => {
                assert_eq!(fee_bps, 30);
                assert_eq!(ticks.len(), 1);
                let tick = &ticks[0];
                assert_eq!(tick.price, Decimal::from(2));
                assert_eq!(tick.liquidity_gross, Decimal::from(5u128));
            }
            _ => panic!("expected ConcentratedLiquidity model"),
        }
    }

    #[test]
    fn test_reserves_from_liquidity_and_sqrt_price() {
        // sqrt_price = 1, liquidity = 1_000_000, decimals = 6
        let (qx, qy) = reserves_from_liquidity_and_sqrt_price(1_000_000u128, 1u128 << 64, 6, 6)
            .expect("reserves calc failed");
        assert_eq!(qx, Quantity(dec!(1)));
        assert_eq!(qy, Quantity(dec!(1)));
    }

    #[test]
    fn test_extreme_values_rejected() {
        // Test that extremely large values are rejected
        let result = reserves_from_liquidity_and_sqrt_price(u128::MAX, u128::MAX / 2, 6, 6);
        assert!(result.is_err());
        
        // Test that zero sqrt_price is rejected
        let result = reserves_from_liquidity_and_sqrt_price(1_000_000, 0, 6, 6);
        assert!(result.is_err());
        
        // Test that extremely small sqrt_price is rejected
        let result = reserves_from_liquidity_and_sqrt_price(1_000_000, 1, 6, 6);
        assert!(result.is_err());
    }

    #[test]
    fn test_reasonable_extreme_values_accepted() {
        // Test high but reasonable values
        let result = reserves_from_liquidity_and_sqrt_price(
            1_000_000_000_000u128, 
            (1u128 << 64) * 1000, 
            6, 
            6
        );
        assert!(result.is_ok());
        
        // Test low but reasonable values
        let result = reserves_from_liquidity_and_sqrt_price(
            1, 
            (1u128 << 64) / 1000, 
            6, 
            6
        );
        assert!(result.is_ok());
    }
}
