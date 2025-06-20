use anyhow::{Context, Result};
use async_trait::async_trait;
use common::types::{Event, MarketUpdate, TickInfo, TokenPair};
use dashmap::DashMap;
use dex_adapter_trait::DexAdapter;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

// --- Data Structures for Deserialization and State ---

/// Represents the data from a swap event.
/// It includes post-swap state to keep the internal model synchronized.
#[derive(Deserialize, Debug, Clone)]
struct SwapEventData {
    pool_id: String,
    // Post-swap state
    sqrt_price: u128,
    liquidity: u128,
    tick: i32,
}

/// Represents a full snapshot of a pool's state.
/// This is used to initialize or fully refresh the state of a pool.
#[derive(Deserialize, Debug, Clone)]
struct PoolSnapshotData {
    pool_id: String,
    sqrt_price: u128,
    liquidity: u128,
    tick: i32,
    fee_rate: u64,
    // Assuming token types are part of the snapshot for creating the TokenPair
    token_a: String,
    token_b: String,
    /// CRITICAL ASSUMPTION: The snapshot must contain the tick map (liquidity distribution).
    /// The arbitrage detector requires this to calculate price impact for different trade sizes.
    #[serde(default)]
    tick_map: HashMap<i32, TickInfo>,
}

/// Holds the internal state for a single liquidity pool.
#[derive(Debug, Clone)]
struct PoolState {
    token_pair: TokenPair,
    sqrt_price: u128,
    liquidity: u128,
    tick: i32,
    fee_bps: u32,
    tick_map: HashMap<i32, TickInfo>,
}

// --- Adapter Implementations ---

#[derive(Default)]
pub struct HyperionAdapter {
    pools: Arc<DashMap<String, PoolState>>,
}

impl HyperionAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DexAdapter for HyperionAdapter {
    fn id(&self) -> &'static str {
        "hyperion"
    }

    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
        // Assuming event type_str is fully qualified, e.g., `0x...::hyperion::SwapEvent`
        let event_name = event.type_str.split("::").last().unwrap_or("");

        match event_name {
            "PoolSnapshot" => {
                let snapshot: PoolSnapshotData = serde_json::from_str(&event.data)
                    .context("Failed to deserialize PoolSnapshotData")?;
                let state = PoolState {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    sqrt_price: snapshot.sqrt_price,
                    liquidity: snapshot.liquidity,
                    tick: snapshot.tick,
                    // Assuming fee_rate is in basis points, e.g., 30 for 0.30%
                    fee_bps: (snapshot.fee_rate) as u32,
                    tick_map: snapshot.tick_map,
                };
                self.pools.insert(snapshot.pool_id, state);
                // A snapshot only updates our internal state; it doesn't trigger a market update.
                Ok(None)
            }
            "SwapEvent" | "SwapAfterEvent" => {
                let swap: SwapEventData = serde_json::from_str(&event.data)
                    .context("Failed to deserialize SwapEventData")?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state) = self.pools.get_mut(&pool_id) {
                    // First, update our internal state to reflect the post-swap reality.
                    pool_state.sqrt_price = swap.sqrt_price;
                    pool_state.liquidity = swap.liquidity;
                    pool_state.tick = swap.tick;

                    // Then, create the market update from the *new* state.
                    let market_update = MarketUpdate {
                        pool_address: pool_id.clone(),
                        dex_name: self.id().to_string(),
                        token_pair: pool_state.token_pair.clone(),
                        sqrt_price: pool_state.sqrt_price,
                        liquidity: pool_state.liquidity,
                        tick: pool_state.tick as u32,
                        fee_bps: pool_state.fee_bps,
                        tick_map: pool_state.tick_map.clone(),
                    };

                    Ok(Some(market_update))
                } else {
                    // We received a swap for a pool we have no prior state for.
                    // We cannot create a valid MarketUpdate without the liquidity distribution.
                    Ok(None)
                }
            }
            _ => {
                // This adapter doesn't care about other event types.
                Ok(None)
            }
        }
    }
}

#[derive(Default)]
pub struct ThalaAdapter {
    pools: Arc<DashMap<String, PoolState>>,
}

impl ThalaAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DexAdapter for ThalaAdapter {
    fn id(&self) -> &'static str {
        "thala"
    }

    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
        // The logic is assumed to be identical to Hyperion's for now.
        // This can be customized if Thala's events differ.
        let event_name = event.type_str.split("::").last().unwrap_or("");

        match event_name {
            "PoolSnapshot" => {
                let snapshot: PoolSnapshotData = serde_json::from_str(&event.data)
                    .context("Failed to deserialize PoolSnapshotData for Thala")?;
                let state = PoolState {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    sqrt_price: snapshot.sqrt_price,
                    liquidity: snapshot.liquidity,
                    tick: snapshot.tick,
                    fee_bps: (snapshot.fee_rate) as u32,
                    tick_map: snapshot.tick_map,
                };
                self.pools.insert(snapshot.pool_id, state);
                Ok(None)
            }
            "SwapEvent" | "SwapAfterEvent" => {
                let swap: SwapEventData = serde_json::from_str(&event.data)
                    .context("Failed to deserialize SwapEventData for Thala")?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state) = self.pools.get_mut(&pool_id) {
                    // First, update our internal state to reflect the post-swap reality.
                    pool_state.sqrt_price = swap.sqrt_price;
                    pool_state.liquidity = swap.liquidity;
                    pool_state.tick = swap.tick;

                    // Then, create the market update from the *new* state.
                    let market_update = MarketUpdate {
                        pool_address: pool_id.clone(),
                        dex_name: self.id().to_string(),
                        token_pair: pool_state.token_pair.clone(),
                        sqrt_price: pool_state.sqrt_price,
                        liquidity: pool_state.liquidity,
                        tick: pool_state.tick as u32,
                        fee_bps: pool_state.fee_bps,
                        tick_map: pool_state.tick_map.clone(),
                    };

                    Ok(Some(market_update))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }
}

pub struct TappAdapter;

#[async_trait]
impl DexAdapter for TappAdapter {
    fn id(&self) -> &'static str {
        "tapp"
    }

    fn parse_event(&self, _event: &Event) -> Result<Option<MarketUpdate>> {
        unimplemented!()
    }
}
