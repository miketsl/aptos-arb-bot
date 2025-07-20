//! Tapp DEX adapter implementation for Aptos arbitrage bot.

use crate::{DexAdapter, PoolState};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use common::types::{
    ClmmMarketUpdate, ConstantProductMarketUpdate, Event, MarketUpdate, StableSwapMarketUpdate,
    TokenPair,
};
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
pub struct TappAdapter {
    module_addresses: Vec<String>,
}

impl TappAdapter {
    pub fn new(module_addresses: Vec<String>) -> Self {
        Self { module_addresses }
    }
}

impl Default for TappAdapter {
    fn default() -> Self {
        Self::new(vec![
            "0xtapp_module_address".to_string(), // Placeholder from config
        ])
    }
}

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

    fn module_addresses(&self) -> &[String] {
        &self.module_addresses
    }

    fn extract_pool_ids(&self, event: &Event) -> Result<Vec<String>> {
        // Try to parse as Tapp swap event to extract pool ID
        if let Ok(swap) = from_slice::<TappSwapEvent>(event.data.as_bytes()) {
            Ok(vec![swap.pool_id])
        } else {
            Ok(vec![])
        }
    }

    async fn fetch_pool_state(&self, pool_id: &str) -> Result<PoolState> {
        // Tapp REST API integration
        let client = reqwest::Client::new();
        let url = format!("https://api.tapp.xyz/v1/pools/{}", pool_id);

        let response = client.get(&url).send().await?;
        let pool_data: TappPoolResponse = response.json().await?;

        // Handle different pool types
        let (reserve_a, reserve_b) = match pool_data.pool_type.as_str() {
            "clmm" => {
                // For CLMM pools, calculate reserves from liquidity and price
                let sqrt_price = pool_data.sqrt_price.unwrap_or(0);
                let liquidity = pool_data.liquidity.unwrap_or(0);
                // Simplified calculation - real implementation would be more complex
                (
                    liquidity.to_string(),
                    (liquidity / sqrt_price.max(1)).to_string(),
                )
            }
            "constant_product" | "stable_swap" => {
                if pool_data.reserves.len() >= 2 {
                    (pool_data.reserves[0].clone(), pool_data.reserves[1].clone())
                } else {
                    return Err(anyhow!("Pool must have at least 2 reserves"));
                }
            }
            _ => return Err(anyhow!("Unsupported pool type: {}", pool_data.pool_type)),
        };

        let (token_a, token_b) = if pool_data.tokens.len() >= 2 {
            (pool_data.tokens[0].clone(), pool_data.tokens[1].clone())
        } else {
            return Err(anyhow!("Pool must have at least 2 tokens"));
        };

        Ok(PoolState {
            pool_id: pool_data.pool_id,
            dex_name: self.id().to_string(),
            token_a,
            token_b,
            reserve_a,
            reserve_b,
            fee_rate: (pool_data.fee_bps as f64 / 10000.0).to_string(),
            block_height: pool_data.last_updated_block,
            additional_data: serde_json::json!({
                "pool_type": pool_data.pool_type,
                "all_tokens": pool_data.tokens,
                "all_reserves": pool_data.reserves,
                "sqrt_price": pool_data.sqrt_price,
                "liquidity": pool_data.liquidity,
                "tick": pool_data.tick,
                "amplification_factor": pool_data.amplification_factor
            }),
        })
    }
}

/// Response structure for Tapp pool state API
#[derive(Debug, Deserialize)]
struct TappPoolResponse {
    pool_id: String,
    pool_type: String,
    tokens: Vec<String>,
    reserves: Vec<String>,
    fee_bps: u32,
    last_updated_block: u64,
    // CLMM specific fields
    sqrt_price: Option<u128>,
    liquidity: Option<u128>,
    tick: Option<i32>,
    // Stable swap specific fields
    amplification_factor: Option<u32>,
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
