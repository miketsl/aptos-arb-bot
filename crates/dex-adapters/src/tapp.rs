//! Tapp DEX adapter implementation for Aptos arbitrage bot.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use common::types::{
    ClmmMarketUpdate, ConstantProductMarketUpdate, Event, MarketUpdate, StableSwapMarketUpdate,
    TokenPair,
};
use dex_adapter_trait::DexAdapter;
use serde::Deserialize;
use serde_json::from_slice;
use std::collections::HashMap;

// Local struct for deserializing Tapp swap events.
// This is a simplified example. A real implementation would need to know the exact
// structure of the event data for different pool types (CLMM, Stable, etc.).
#[derive(Debug, Deserialize)]
struct TappSwapEvent {
    pool_id: String,
    pool_type: String, // "clmm", "constant_product", "stable_swap"
    token_in: String,
    token_out: String,
    // The event should provide the final state, not just deltas.
    sqrt_price_after: Option<u128>,
    liquidity_after: Option<u128>,
    tick_after: Option<i32>,
    reserve_x_after: Option<String>, // Using String to parse into Decimal
    reserve_y_after: Option<String>,
    amplification_factor: Option<u128>,
    reserves_after: Option<Vec<String>>,
    fee_bps: u32,
}

/// A stateless adapter for the Tapp DEX.
#[derive(Default)]
pub struct TappAdapter;

#[async_trait]
impl DexAdapter for TappAdapter {
    fn id(&self) -> &'static str {
        "tapp"
    }

    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
        let swap: TappSwapEvent = match from_slice(event.data.as_bytes()) {
            Ok(s) => s,
            Err(_) => return Ok(None), // Not a Tapp swap event
        };

        let token_pair = TokenPair {
            token0: swap.token_in.clone(),
            token1: swap.token_out.clone(),
        };

        let market_update = match swap.pool_type.as_str() {
            "clmm" => {
                let sqrt_price = swap
                    .sqrt_price_after
                    .ok_or_else(|| anyhow!("Missing sqrt_price_after for CLMM"))?;
                let liquidity = swap
                    .liquidity_after
                    .ok_or_else(|| anyhow!("Missing liquidity_after for CLMM"))?;
                let tick = swap
                    .tick_after
                    .ok_or_else(|| anyhow!("Missing tick_after for CLMM"))?;

                MarketUpdate::Clmm(ClmmMarketUpdate {
                    pool_address: swap.pool_id,
                    dex_name: self.id().to_string(),
                    token_pair,
                    sqrt_price,
                    liquidity,
                    tick,
                    fee_bps: swap.fee_bps,
                    tick_map: HashMap::new(),
                })
            }
            "constant_product" => {
                let reserve_x_str = swap
                    .reserve_x_after
                    .ok_or_else(|| anyhow!("Missing reserve_x_after"))?;
                let reserve_y_str = swap
                    .reserve_y_after
                    .ok_or_else(|| anyhow!("Missing reserve_y_after"))?;
                let reserve_x = reserve_x_str
                    .parse()
                    .map_err(|e| anyhow!("Failed to parse reserve_x: {}", e))?;
                let reserve_y = reserve_y_str
                    .parse()
                    .map_err(|e| anyhow!("Failed to parse reserve_y: {}", e))?;

                MarketUpdate::ConstantProduct(ConstantProductMarketUpdate {
                    pool_address: swap.pool_id,
                    dex_name: self.id().to_string(),
                    token_pair,
                    reserve_x: common::types::Quantity(reserve_x),
                    reserve_y: common::types::Quantity(reserve_y),
                    fee_bps: swap.fee_bps,
                })
            }
            "stable_swap" => {
                let reserves_str = swap
                    .reserves_after
                    .ok_or_else(|| anyhow!("Missing reserves_after"))?;
                let reserves = reserves_str
                    .into_iter()
                    .map(|s| s.parse().map(common::types::Quantity))
                    .collect::<Result<Vec<_>, _>>()?;
                let amplification_factor = swap
                    .amplification_factor
                    .ok_or_else(|| anyhow!("Missing amplification_factor"))?;

                MarketUpdate::StableSwap(StableSwapMarketUpdate {
                    pool_address: swap.pool_id,
                    dex_name: self.id().to_string(),
                    token_pair,
                    reserves,
                    amplification_factor,
                    fee_bps: swap.fee_bps,
                })
            }
            _ => return Ok(None), // Unsupported pool type
        };

        Ok(Some(market_update))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Event;
    
    use rust_decimal_macros::dec;

    #[test]
    fn test_parse_tapp_clmm_event() {
        let adapter = TappAdapter::default();
        let event_data = serde_json::json!({
            "pool_id": "0x456",
            "pool_type": "clmm",
            "token_in": "0x1::aptos_coin::AptosCoin",
            "token_out": "0x2::usdc::USDC",
            "sqrt_price_after": 1_234_567_890_123_456_789u128,
            "liquidity_after": 1_000_000_000u128,
            "tick_after": 54321,
            "fee_bps": 25
        });

        let event = Event {
            data: event_data.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_some());

        if let Some(MarketUpdate::Clmm(update)) = result {
            assert_eq!(update.pool_address, "0x456");
            assert_eq!(update.dex_name, "tapp");
            assert_eq!(update.sqrt_price, 1_234_567_890_123_456_789);
            assert_eq!(update.tick, 54321);
        } else {
            panic!("Expected ClmmMarketUpdate");
        }
    }

    #[test]
    fn test_parse_tapp_cpmm_event() {
        let adapter = TappAdapter::default();
        let event_data = serde_json::json!({
            "pool_id": "0x789",
            "pool_type": "constant_product",
            "token_in": "0x1::aptos_coin::AptosCoin",
            "token_out": "0x3::usdt::USDT",
            "reserve_x_after": "10000.5",
            "reserve_y_after": "5000.25",
            "fee_bps": 30
        });

        let event = Event {
            data: event_data.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_some());

        if let Some(MarketUpdate::ConstantProduct(update)) = result {
            assert_eq!(update.pool_address, "0x789");
            assert_eq!(update.dex_name, "tapp");
            assert_eq!(update.reserve_x.0, dec!(10000.5));
            assert_eq!(update.reserve_y.0, dec!(5000.25));
        } else {
            panic!("Expected ConstantProductMarketUpdate");
        }
    }

    #[test]
    fn test_ignore_unknown_pool_type() {
        let adapter = TappAdapter::default();
        let event_data = serde_json::json!({
            "pool_id": "0xabc",
            "pool_type": "unknown_type",
            "token_in": "0x1::aptos_coin::AptosCoin",
            "token_out": "0x2::usdc::USDC",
            "fee_bps": 30
        });

        let event = Event {
            data: event_data.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_none());
    }
}
