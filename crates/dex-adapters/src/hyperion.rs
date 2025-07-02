//! Hyperion DEX adapter implementation for Aptos arbitrage bot

use anyhow::Result;
use async_trait::async_trait;
use common::types::{ClmmMarketUpdate, Event, MarketUpdate, TokenPair};
use dex_adapter_trait::DexAdapter;
use serde::Deserialize;
use serde_json::from_slice;
use std::collections::HashMap;

// This local struct replaces the need for the shared `types.rs`.
// It is specific to the data format of Hyperion's swap events.
// NOTE: This assumes the event data contains the final state of the pool after the swap.
// If it only contains deltas, the event producer (MDI) needs to be adjusted.
#[derive(Debug, Deserialize)]
struct HyperionSwapEvent {
    pool_id: String,
    token_a: String,
    token_b: String,
    sqrt_price_after: u128,
    liquidity_after: u128,
    tick_after: i32,
    fee_rate: u32,
}

/// A stateless adapter for the Hyperion DEX.
#[derive(Default)]
pub struct HyperionAdapter;

#[async_trait]
impl DexAdapter for HyperionAdapter {
    fn id(&self) -> &'static str {
        "hyperion"
    }

    /// Parses a raw event and returns a `MarketUpdate` if the event is a valid Hyperion swap.
    /// This function is a pure, stateless transformation.
    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
        // Attempt to deserialize the event data into our specific struct.
        // If it fails, it's not a Hyperion swap event we're interested in, so we ignore it.
        let swap: HyperionSwapEvent = match from_slice(event.data.as_bytes()) {
            Ok(s) => s,
            Err(_) => return Ok(None),
        };

        // Directly translate the event data into the canonical `MarketUpdate` format.
        let market_update = MarketUpdate::Clmm(ClmmMarketUpdate {
            pool_address: swap.pool_id,
            dex_name: self.id().to_string(),
            token_pair: TokenPair {
                token0: swap.token_a,
                token1: swap.token_b,
            },
            sqrt_price: swap.sqrt_price_after,
            liquidity: swap.liquidity_after,
            tick: swap.tick_after,
            fee_bps: swap.fee_rate,
            tick_map: HashMap::new(), // The tick_map is not part of a single swap event and is managed by MDI/Detector.
        });

        Ok(Some(market_update))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Event;

    #[test]
    fn test_parse_hyperion_swap_event() {
        let adapter = HyperionAdapter::default();
        let event_data = serde_json::json!({
            "pool_id": "0x123",
            "token_a": "0x1::aptos_coin::AptosCoin",
            "token_b": "0x2::usdc::USDC",
            "sqrt_price_after": 1_234_567_890_123_456_789u128,
            "liquidity_after": 1_000_000_000u128,
            "tick_after": 12345,
            "fee_rate": 30
        });

        let event = Event {
            data: event_data.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_some());

        if let Some(MarketUpdate::Clmm(update)) = result {
            assert_eq!(update.pool_address, "0x123");
            assert_eq!(update.dex_name, "hyperion");
            assert_eq!(update.token_pair.token0, "0x1::aptos_coin::AptosCoin");
            assert_eq!(update.token_pair.token1, "0x2::usdc::USDC");
            assert_eq!(update.sqrt_price, 1_234_567_890_123_456_789);
            assert_eq!(update.liquidity, 1_000_000_000);
            assert_eq!(update.tick, 12345);
            assert_eq!(update.fee_bps, 30);
        } else {
            panic!("Expected ClmmMarketUpdate");
        }
    }

    #[test]
    fn test_ignore_irrelevant_event() {
        let adapter = HyperionAdapter::default();
        let event = Event {
            data: r#"{ "some": "data" }"#.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_malformed_json() {
        let adapter = HyperionAdapter::default();
        let event = Event {
            data: r#"{ "invalid_json": "..." "#.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_none());
    }
}
