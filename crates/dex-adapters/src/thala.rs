//! Thala DEX adapter implementation for Aptos arbitrage bot.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use common::types::{Event, MarketUpdate, TokenPair, WeightedPoolMarketUpdate};
use dex_adapter_trait::DexAdapter;
use serde::Deserialize;
use serde_json::from_slice;

// Local struct for deserializing Thala swap events.
// This is a simplified example for a weighted pool. Thala may have different
// event structures for different pool types.
#[derive(Debug, Deserialize)]
struct ThalaSwapEvent {
    pool_id: String,
    pool_type: String, // e.g., "weighted"
    tokens_in: Vec<String>,
    tokens_out: Vec<String>,
    // The event should provide the final state of all reserves and weights.
    reserves_after: Vec<String>, // Using String to parse into Decimal
    weights: Vec<u32>,
    fee_bps: u32,
}

/// A stateless adapter for the Thala DEX.
#[derive(Default)]
pub struct ThalaAdapter;

#[async_trait]
impl DexAdapter for ThalaAdapter {
    fn id(&self) -> &'static str {
        "thala"
    }

    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
        let swap: ThalaSwapEvent = match from_slice(event.data.as_bytes()) {
            Ok(s) => s,
            Err(_) => return Ok(None), // Not a Thala swap event
        };

        // This adapter will only handle weighted pools as an example.
        if swap.pool_type != "weighted" {
            return Ok(None);
        }

        // Basic validation
        if swap.tokens_in.len() != 1 || swap.tokens_out.len() != 1 {
            return Err(anyhow!("Multi-token swaps not supported by this adapter"));
        }
        if swap.reserves_after.len() != swap.weights.len() {
            return Err(anyhow!("Mismatched reserves and weights length"));
        }

        let token_pair = TokenPair {
            token0: swap.tokens_in[0].clone(),
            token1: swap.tokens_out[0].clone(),
        };

        let reserves = swap
            .reserves_after
            .into_iter()
            .map(|s| s.parse().map(common::types::Quantity))
            .collect::<Result<Vec<_>, _>>()?;

        let market_update = MarketUpdate::WeightedPool(WeightedPoolMarketUpdate {
            pool_address: swap.pool_id,
            dex_name: self.id().to_string(),
            token_pair,
            reserves,
            weights: swap.weights,
            fee_bps: swap.fee_bps,
        });

        Ok(Some(market_update))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Event;

    use rust_decimal_macros::dec;

    #[test]
    fn test_parse_thala_weighted_pool_event() {
        let adapter = ThalaAdapter::default();
        let event_data = serde_json::json!({
            "pool_id": "0xabc",
            "pool_type": "weighted",
            "tokens_in": ["0x1::aptos_coin::AptosCoin"],
            "tokens_out": ["0x2::usdc::USDC"],
            "reserves_after": ["1000.0", "20000.0"],
            "weights": [500000, 500000], // 50/50
            "fee_bps": 10
        });

        let event = Event {
            data: event_data.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_some());

        if let Some(MarketUpdate::WeightedPool(update)) = result {
            assert_eq!(update.pool_address, "0xabc");
            assert_eq!(update.dex_name, "thala");
            assert_eq!(update.reserves.len(), 2);
            assert_eq!(update.reserves[0].0, dec!(1000.0));
            assert_eq!(update.reserves[1].0, dec!(20000.0));
            assert_eq!(update.weights.len(), 2);
            assert_eq!(update.weights[0], 500000);
            assert_eq!(update.fee_bps, 10);
        } else {
            panic!("Expected WeightedPoolMarketUpdate");
        }
    }

    #[test]
    fn test_ignore_other_pool_types() {
        let adapter = ThalaAdapter::default();
        let event_data = serde_json::json!({
            "pool_id": "0xdef",
            "pool_type": "stable", // Not "weighted"
            "tokens_in": ["0x1::aptos_coin::AptosCoin"],
            "tokens_out": ["0x2::usdc::USDC"],
            "reserves_after": ["1000.0", "1000.0"],
            "fee_bps": 4
        });

        let event = Event {
            data: event_data.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_malformed_thala_event() {
        let adapter = ThalaAdapter::default();
        let event_data = serde_json::json!({
            "pool_id": "0xghi",
            "pool_type": "weighted",
            "tokens_in": ["0x1::aptos_coin::AptosCoin"],
            "tokens_out": ["0x2::usdc::USDC"],
            "reserves_after": ["1000.0"], // Mismatched length
            "weights": [500000, 500000],
            "fee_bps": 10
        });

        let event = Event {
            data: event_data.to_string(),
            ..Default::default()
        };

        let result = adapter.parse_event(&event);
        assert!(result.is_err());
    }
}
