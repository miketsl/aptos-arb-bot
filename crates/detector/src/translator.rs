use crate::graph::{Edge, PoolModel, Tick};
use common::types::{Asset, MarketUpdate, TradingPair};
use dex_adapter_trait::Exchange;
use rust_decimal::{Decimal, MathematicalOps};
use std::str::FromStr;
use std::time::Instant;

/// Converts a `MarketUpdate` from the Market Data Ingestor into an `Edge` for the price graph.
///
/// The function handles the translation of asset types, DEX names, and tick data into the
/// `ConcentratedLiquidity` model required by the detector's graph.
pub fn market_update_to_edge(market_update: &MarketUpdate) -> Result<Edge, anyhow::Error> {
    // 1. Convert string-based token pairs to Asset types.
    let asset_x = Asset::from_str(&market_update.token_pair.token0)?;
    let asset_y = Asset::from_str(&market_update.token_pair.token1)?;

    // 2. Map the DEX name string to the Exchange enum.
    let exchange = Exchange::from_str(&market_update.dex_name)?;

    // 3. Convert the tick map into a vector of `Tick` structs for the CLMM model.
    // The price at each tick is calculated as p(i) = 1.0001^i.
    let ticks: Vec<Tick> = market_update
        .tick_map
        .iter()
        .map(|(tick_index, tick_info)| {
            // Price calculation: p(i) = 1.0001^i
            // Using Decimal::powd for precision.
            let price = Decimal::from_str("1.0001")
                .unwrap()
                .powd(Decimal::from(*tick_index));

            // Convert liquidity from u128 to Decimal.
            let liquidity_gross = Decimal::from(tick_info.liquidity_gross);

            Tick {
                price,
                liquidity_gross,
            }
        })
        .collect();

    // 4. Construct the Edge.
    let edge = Edge {
        pair: TradingPair { asset_x, asset_y },
        exchange,
        model: PoolModel::ConcentratedLiquidity {
            ticks,
            fee_bps: market_update.fee_bps as u16, // Safely cast from u32 to u16
        },
        last_updated: Instant::now(),
    };

    Ok(edge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{MarketUpdate, TickInfo, TokenPair};
    use rust_decimal::MathematicalOps;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    #[test]
    fn test_market_update_to_edge_conversion() {
        // 1. Create a sample MarketUpdate.
        let mut tick_map = HashMap::new();
        tick_map.insert(
            -20,
            TickInfo {
                liquidity_net: 1000,
                liquidity_gross: 10000,
            },
        );
        tick_map.insert(
            10,
            TickInfo {
                liquidity_net: -500,
                liquidity_gross: 5000,
            },
        );

        let market_update = MarketUpdate {
            pool_address: "0x1234".to_string(),
            dex_name: "Tapp".to_string(),
            token_pair: TokenPair {
                token0: "0x1::aptos_coin::AptosCoin".to_string(),
                token1: "0x1::coin::USDC".to_string(),
            },
            sqrt_price: 123456789,
            liquidity: 100000,
            tick: 123,
            fee_bps: 30,
            tick_map,
        };

        // 2. Perform the conversion.
        let result = market_update_to_edge(&market_update);
        assert!(result.is_ok());
        let edge = result.unwrap();

        // 3. Assert the fields of the resulting Edge.
        let expected_asset_x = Asset::from_str("0x1::aptos_coin::AptosCoin").unwrap();
        let expected_asset_y = Asset::from_str("0x1::coin::USDC").unwrap();

        assert_eq!(edge.pair.asset_x, expected_asset_x);
        assert_eq!(edge.pair.asset_y, expected_asset_y);
        assert_eq!(edge.exchange, Exchange::Tapp);
        assert_eq!(edge.model.fee_bps(), 30);

        // Check the ConcentratedLiquidity model and its ticks.
        if let PoolModel::ConcentratedLiquidity { ticks, fee_bps } = edge.model {
            assert_eq!(fee_bps, 30);
            assert_eq!(ticks.len(), 2);

            // Verify tick for index -20
            let tick_minus_20 = ticks
                .iter()
                .find(|t| {
                    let expected_price = dec!(1.0001).powd(dec!(-20));
                    // Compare with a tolerance for floating point inaccuracies
                    (t.price - expected_price).abs() < dec!(1e-12)
                })
                .unwrap();
            assert_eq!(tick_minus_20.liquidity_gross, dec!(10000));

            // Verify tick for index 10
            let tick_10 = ticks
                .iter()
                .find(|t| {
                    let expected_price = dec!(1.0001).powd(dec!(10));
                    (t.price - expected_price).abs() < dec!(1e-12)
                })
                .unwrap();
            assert_eq!(tick_10.liquidity_gross, dec!(5000));
        } else {
            panic!("Expected ConcentratedLiquidity model");
        }
    }

    #[test]
    fn test_invalid_dex_name() {
        let market_update = MarketUpdate {
            pool_address: "0x1234".to_string(),
            dex_name: "InvalidDex".to_string(),
            token_pair: TokenPair {
                token0: "0x1::aptos_coin::AptosCoin".to_string(),
                token1: "0x1::coin::USDC".to_string(),
            },
            sqrt_price: 123456789,
            liquidity: 100000,
            tick: 123,
            fee_bps: 30,
            tick_map: HashMap::new(),
        };

        let result = market_update_to_edge(&market_update);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid exchange: InvalidDex"
        );
    }
}
