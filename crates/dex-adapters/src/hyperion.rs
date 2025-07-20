//! Hyperion DEX adapter implementation for Aptos arbitrage bot

use anyhow::Result;
use async_trait::async_trait;
use common::types::{ClmmMarketUpdate, Event, MarketUpdate, TokenPair};
use crate::{DexAdapter, PoolState};
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
pub struct HyperionAdapter {
    module_addresses: Vec<String>,
}

impl HyperionAdapter {
    pub fn new(module_addresses: Vec<String>) -> Self {
        Self { module_addresses }
    }
}

impl Default for HyperionAdapter {
    fn default() -> Self {
        Self::new(vec![
            "0x89576037b3cc0b89645ea393a47787bb348272c76d6941c574b053672b848039".to_string()
        ])
    }
}

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

    fn module_addresses(&self) -> &[String] {
        &self.module_addresses
    }

    fn extract_pool_ids(&self, event: &Event) -> Result<Vec<String>> {
        // Try to parse as Hyperion swap event to extract pool ID
        if let Ok(swap) = from_slice::<HyperionSwapEvent>(event.data.as_bytes()) {
            Ok(vec![swap.pool_id])
        } else {
            Ok(vec![])
        }
    }

    async fn fetch_pool_state(&self, pool_id: &str) -> Result<PoolState> {
        // Hyperion REST API integration
        let client = reqwest::Client::new();
        let url = format!("https://api.hyperion.xyz/v1/pools/{}", pool_id);
        
        let response = client.get(&url).send().await?;
        let pool_data: HyperionPoolResponse = response.json().await?;
        
        Ok(PoolState {
            pool_id: pool_data.pool_id,
            dex_name: self.id().to_string(),
            token_a: pool_data.token_a,
            token_b: pool_data.token_b,
            reserve_a: pool_data.reserve_a.to_string(),
            reserve_b: pool_data.reserve_b.to_string(),
            fee_rate: (pool_data.fee_bps as f64 / 10000.0).to_string(),
            block_height: pool_data.last_updated_block,
            additional_data: serde_json::json!({
                "pool_type": "clmm",
                "sqrt_price": pool_data.sqrt_price,
                "liquidity": pool_data.liquidity,
                "tick": pool_data.tick,
                "tick_spacing": pool_data.tick_spacing
            }),
        })
    }
}

/// Response structure for Hyperion pool state API
#[derive(Debug, Deserialize)]
struct HyperionPoolResponse {
    pool_id: String,
    token_a: String,
    token_b: String,
    reserve_a: u128,
    reserve_b: u128,
    fee_bps: u32,
    sqrt_price: u128,
    liquidity: u128,
    tick: i32,
    tick_spacing: u32,
    last_updated_block: u64,
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
