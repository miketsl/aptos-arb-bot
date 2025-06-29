use anyhow::{Context, Result};
use async_trait::async_trait;
use common::types::{
    ClmmMarketUpdate, ConstantProductMarketUpdate, Event, MarketUpdate, Quantity,
    StableSwapMarketUpdate, TickInfo, TokenPair, WeightedPoolMarketUpdate,
};
use dashmap::DashMap;
use dex_adapter_trait::DexAdapter;
use serde::Deserialize;
use simd_json::serde::from_slice as simd_from_slice;
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

// For Constant Product Market Makers (CPMM)
#[derive(Deserialize, Debug, Clone)]
struct AmmPoolSnapshotData {
    pool_id: String,
    token_a: String,
    token_b: String,
    reserve_x: Quantity,
    reserve_y: Quantity,
    fee_bps: u32,
}

// For StableSwap pools
#[derive(Deserialize, Debug, Clone)]
struct StableSwapPoolSnapshotData {
    pool_id: String,
    token_a: String,
    token_b: String,
    reserves: Vec<Quantity>,
    amplification_factor: u128,
    fee_bps: u32,
}

// For Weighted Pools
#[derive(Deserialize, Debug, Clone)]
struct WeightedPoolSnapshotData {
    pool_id: String,
    token_a: String,
    token_b: String,
    reserves: Vec<Quantity>,
    weights: Vec<u32>,
    fee_bps: u32,
}

/// Holds the internal state for a single liquidity pool.
#[derive(Debug, Clone)]
enum PoolState {
    Clmm {
        token_pair: TokenPair,
        sqrt_price: u128,
        liquidity: u128,
        tick: i32,
        fee_bps: u32,
        tick_map: HashMap<i32, TickInfo>,
    },
    ConstantProduct {
        token_pair: TokenPair,
        reserve_x: Quantity,
        reserve_y: Quantity,
        fee_bps: u32,
    },
    StableSwap {
        token_pair: TokenPair,
        reserves: Vec<Quantity>,
        amplification_factor: u128,
        fee_bps: u32,
    },
    WeightedPool {
        token_pair: TokenPair,
        reserves: Vec<Quantity>,
        weights: Vec<u32>,
        fee_bps: u32,
    },
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
                // Use simd-json for faster in-place JSON parsing
                let mut raw = event.data.clone().into_bytes();
                let snapshot: PoolSnapshotData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize PoolSnapshotData with simd-json")?;
                let state = PoolState::Clmm {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    sqrt_price: snapshot.sqrt_price,
                    liquidity: snapshot.liquidity,
                    tick: snapshot.tick,
                    // Assuming fee_rate is in basis points, e.g., 30 for 0.30%
                    fee_bps: snapshot.fee_rate as u32,
                    tick_map: snapshot.tick_map,
                };
                self.pools.insert(snapshot.pool_id, state);
                // A snapshot only updates our internal state; it doesn't trigger a market update.
                Ok(None)
            }
            "SwapEvent" | "SwapAfterEvent" => {
                let mut raw = event.data.clone().into_bytes();
                let swap: SwapEventData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize SwapEventData with simd-json")?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state_guard) = self.pools.get_mut(&pool_id) {
                    // Extract immutable data first
                    let token_pair = pool_state_guard.token_pair().clone();
                    let fee_bps = pool_state_guard.fee_bps();
                    let tick_map = pool_state_guard.tick_map().clone();

                    // Now, get a mutable reference to the inner PoolState enum variant
                    if let PoolState::Clmm {
                        sqrt_price,
                        liquidity,
                        tick,
                        ..
                    } = &mut *pool_state_guard
                    {
                        *sqrt_price = swap.sqrt_price;
                        *liquidity = swap.liquidity;
                        *tick = swap.tick;

                        let market_update = MarketUpdate::Clmm(ClmmMarketUpdate {
                            pool_address: pool_id.clone(),
                            dex_name: self.id().to_string(),
                            token_pair,
                            sqrt_price: *sqrt_price,
                            liquidity: *liquidity,
                            tick: *tick,
                            fee_bps,
                            tick_map,
                        });

                        Ok(Some(market_update))
                    } else {
                        eprintln!("Received swap for non-CLMM pool in HyperionAdapter");
                        Ok(None)
                    }
                } else {
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
        let event_name = event.type_str.split("::").last().unwrap_or("");

        match event_name {
            "PoolSnapshot" => {
                let mut raw = event.data.clone().into_bytes();
                let snapshot: PoolSnapshotData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize PoolSnapshotData for Thala with simd-json")?;
                let state = PoolState::Clmm {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    sqrt_price: snapshot.sqrt_price,
                    liquidity: snapshot.liquidity,
                    tick: snapshot.tick,
                    fee_bps: snapshot.fee_rate as u32,
                    tick_map: snapshot.tick_map,
                };
                self.pools.insert(snapshot.pool_id, state);
                Ok(None)
            }
            "SwapEvent" | "SwapAfterEvent" => {
                let mut raw = event.data.clone().into_bytes();
                let swap: SwapEventData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize SwapEventData for Thala with simd-json")?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state_guard) = self.pools.get_mut(&pool_id) {
                    // Extract immutable data first
                    let token_pair = pool_state_guard.token_pair().clone();
                    let fee_bps = pool_state_guard.fee_bps();
                    let tick_map = pool_state_guard.tick_map().clone();

                    if let PoolState::Clmm {
                        sqrt_price,
                        liquidity,
                        tick,
                        ..
                    } = &mut *pool_state_guard
                    {
                        *sqrt_price = swap.sqrt_price;
                        *liquidity = swap.liquidity;
                        *tick = swap.tick;

                        let market_update = MarketUpdate::Clmm(ClmmMarketUpdate {
                            pool_address: pool_id.clone(),
                            dex_name: self.id().to_string(),
                            token_pair,
                            sqrt_price: *sqrt_price,
                            liquidity: *liquidity,
                            tick: *tick,
                            fee_bps,
                            tick_map,
                        });

                        Ok(Some(market_update))
                    } else {
                        eprintln!("Received swap for non-CLMM pool in ThalaAdapter");
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            // Thala Weighted Pool events
            "WeightedPoolSnapshot" => {
                let mut raw = event.data.clone().into_bytes();
                let snapshot: WeightedPoolSnapshotData = simd_from_slice(&mut raw).context(
                    "Failed to deserialize WeightedPoolSnapshotData for Thala with simd-json",
                )?;
                let state = PoolState::WeightedPool {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    reserves: snapshot.reserves,
                    weights: snapshot.weights,
                    fee_bps: snapshot.fee_bps,
                };
                self.pools.insert(snapshot.pool_id, state);
                Ok(None)
            }
            "WeightedPoolSwap" => {
                // Assuming WeightedPoolSwapEventData is similar to SwapEventData but for weighted pools
                #[derive(Deserialize, Debug, Clone)]
                struct WeightedPoolSwapEventData {
                    pool_id: String,
                    reserves: Vec<Quantity>,
                }
                let mut raw = event.data.clone().into_bytes();
                let swap: WeightedPoolSwapEventData = simd_from_slice(&mut raw).context(
                    "Failed to deserialize WeightedPoolSwapEventData for Thala with simd-json",
                )?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state_guard) = self.pools.get_mut(&pool_id) {
                    let token_pair = pool_state_guard.token_pair().clone();
                    let fee_bps = pool_state_guard.fee_bps();
                    let weights = pool_state_guard.weights().clone();

                    if let PoolState::WeightedPool { reserves, .. } = &mut *pool_state_guard {
                        *reserves = swap.reserves;

                        let market_update = MarketUpdate::WeightedPool(WeightedPoolMarketUpdate {
                            pool_address: pool_id.clone(),
                            dex_name: self.id().to_string(),
                            token_pair,
                            reserves: reserves.clone(),
                            weights,
                            fee_bps,
                        });
                        Ok(Some(market_update))
                    } else {
                        eprintln!("Received swap for non-WeightedPool in ThalaAdapter");
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }
}

#[derive(Default)]
pub struct TappAdapter {
    pools: Arc<DashMap<String, PoolState>>,
}

impl TappAdapter {
    pub fn new() -> Self {
        Self {
            pools: Arc::new(DashMap::new()),
        }
    }
}

#[async_trait]
impl DexAdapter for TappAdapter {
    fn id(&self) -> &'static str {
        "tapp"
    }

    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
        let event_name = event.type_str.split("::").last().unwrap_or("");

        match event_name {
            "PoolSnapshot" => {
                let mut raw = event.data.clone().into_bytes();
                let snapshot: PoolSnapshotData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize PoolSnapshotData for Tapp with simd-json")?;
                let state = PoolState::Clmm {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    sqrt_price: snapshot.sqrt_price,
                    liquidity: snapshot.liquidity,
                    tick: snapshot.tick,
                    fee_bps: snapshot.fee_rate as u32,
                    tick_map: snapshot.tick_map,
                };
                self.pools.insert(snapshot.pool_id, state);
                Ok(None)
            }
            "SwapEvent" | "SwapAfterEvent" => {
                let mut raw = event.data.clone().into_bytes();
                let swap: SwapEventData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize SwapEventData for Tapp with simd-json")?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state) = self.pools.get_mut(&pool_id) {
                    let token_pair = pool_state.token_pair().clone();
                    let fee_bps = pool_state.fee_bps();
                    let tick_map = pool_state.tick_map().clone();
                    if let PoolState::Clmm {
                        sqrt_price,
                        liquidity,
                        tick,
                        ..
                    } = &mut *pool_state
                    {
                        *sqrt_price = swap.sqrt_price;
                        *liquidity = swap.liquidity;
                        *tick = swap.tick;

                        let market_update = MarketUpdate::Clmm(ClmmMarketUpdate {
                            pool_address: pool_id.clone(),
                            dex_name: self.id().to_string(),
                            token_pair,
                            sqrt_price: *sqrt_price,
                            liquidity: *liquidity,
                            tick: *tick,
                            fee_bps,
                            tick_map,
                        });

                        Ok(Some(market_update))
                    } else {
                        eprintln!("Received swap for non-CLMM pool in TappAdapter");
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            // Assuming Tapp also has Constant Product pools
            "AmmPoolSnapshot" => {
                let mut raw = event.data.clone().into_bytes();
                let snapshot: AmmPoolSnapshotData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize AmmPoolSnapshotData for Tapp with simd-json")?;
                let state = PoolState::ConstantProduct {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    reserve_x: snapshot.reserve_x,
                    reserve_y: snapshot.reserve_y,
                    fee_bps: snapshot.fee_bps,
                };
                self.pools.insert(snapshot.pool_id, state);
                Ok(None)
            }
            "AmmSwapEvent" => {
                #[derive(Deserialize, Debug, Clone)]
                struct AmmSwapEventData {
                    pool_id: String,
                    reserves: Vec<Quantity>,
                }
                let mut raw = event.data.clone().into_bytes();
                let swap: AmmSwapEventData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize AmmSwapEventData for Tapp with simd-json")?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state) = self.pools.get_mut(&pool_id) {
                    let token_pair = pool_state.token_pair().clone();
                    let fee_bps = pool_state.fee_bps();
                    if let PoolState::ConstantProduct {
                        reserve_x,
                        reserve_y,
                        ..
                    } = &mut *pool_state
                    {
                        *reserve_x = swap.reserves[0];
                        *reserve_y = swap.reserves[1];

                        let market_update =
                            MarketUpdate::ConstantProduct(ConstantProductMarketUpdate {
                                pool_address: pool_id.clone(),
                                dex_name: self.id().to_string(),
                                token_pair,
                                reserve_x: *reserve_x,
                                reserve_y: *reserve_y,
                                fee_bps,
                            });
                        Ok(Some(market_update))
                    } else {
                        eprintln!("Received swap for non-ConstantProduct pool in TappAdapter");
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            // Assuming Tapp also has StableSwap pools
            "StableSwapPoolSnapshot" => {
                let mut raw = event.data.clone().into_bytes();
                let snapshot: StableSwapPoolSnapshotData = simd_from_slice(&mut raw).context(
                    "Failed to deserialize StableSwapPoolSnapshotData for Tapp with simd-json",
                )?;
                let state = PoolState::StableSwap {
                    token_pair: TokenPair {
                        token0: snapshot.token_a,
                        token1: snapshot.token_b,
                    },
                    reserves: snapshot.reserves,
                    amplification_factor: snapshot.amplification_factor,
                    fee_bps: snapshot.fee_bps,
                };
                self.pools.insert(snapshot.pool_id, state);
                Ok(None)
            }
            "StableSwapEvent" => {
                #[derive(Deserialize, Debug, Clone)]
                struct StableSwapEventData {
                    pool_id: String,
                    reserves: Vec<Quantity>,
                }
                let mut raw = event.data.clone().into_bytes();
                let swap: StableSwapEventData = simd_from_slice(&mut raw)
                    .context("Failed to deserialize StableSwapEventData for Tapp with simd-json")?;
                let pool_id = swap.pool_id.clone();

                if let Some(mut pool_state) = self.pools.get_mut(&pool_id) {
                    let token_pair = pool_state.token_pair().clone();
                    let fee_bps = pool_state.fee_bps();
                    let amplification_factor = pool_state.amplification_factor().unwrap();
                    if let PoolState::StableSwap { reserves, .. } = &mut *pool_state {
                        *reserves = swap.reserves;

                        let market_update = MarketUpdate::StableSwap(StableSwapMarketUpdate {
                            pool_address: pool_id.clone(),
                            dex_name: self.id().to_string(),
                            token_pair,
                            reserves: reserves.clone(),
                            amplification_factor,
                            fee_bps,
                        });
                        Ok(Some(market_update))
                    } else {
                        eprintln!("Received swap for non-StableSwap pool in TappAdapter");
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }
}

impl PoolState {
    fn token_pair(&self) -> &TokenPair {
        match self {
            PoolState::Clmm { token_pair, .. } => token_pair,
            PoolState::ConstantProduct { token_pair, .. } => token_pair,
            PoolState::StableSwap { token_pair, .. } => token_pair,
            PoolState::WeightedPool { token_pair, .. } => token_pair,
        }
    }

    fn fee_bps(&self) -> u32 {
        match self {
            PoolState::Clmm { fee_bps, .. } => *fee_bps,
            PoolState::ConstantProduct { fee_bps, .. } => *fee_bps,
            PoolState::StableSwap { fee_bps, .. } => *fee_bps,
            PoolState::WeightedPool { fee_bps, .. } => *fee_bps,
        }
    }

    fn tick_map(&self) -> &HashMap<i32, TickInfo> {
        if let PoolState::Clmm { tick_map, .. } = self {
            tick_map
        } else {
            panic!("tick_map only available for Clmm pool state");
        }
    }

    fn weights(&self) -> &Vec<u32> {
        if let PoolState::WeightedPool { weights, .. } = self {
            weights
        } else {
            panic!("weights only available for WeightedPool pool state");
        }
    }

    fn amplification_factor(&self) -> Option<u128> {
        if let PoolState::StableSwap {
            amplification_factor,
            ..
        } = self
        {
            Some(*amplification_factor)
        } else {
            None
        }
    }
}
