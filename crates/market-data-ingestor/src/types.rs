use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token pair for CLMM pools
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TokenPair {
    pub token0: String,
    pub token1: String,
}

/// Configuration for a single DEX
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexConfig {
    pub name: String,
    pub module_address: String,
    pub pool_snapshot_event_name: String,
    pub swap_event_name: String,
    pub pools: Vec<String>,
}

/// Configuration for the market data ingestor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataIngestorConfig {
    pub dexs: Vec<DexConfig>,
}

/// Internal representation of a CLMM pool's state
#[derive(Debug, Clone)]
pub struct PoolState {
    pub pool_address: String,
    pub dex_name: String,
    pub sqrt_price: u128,
    pub liquidity: u128,
    pub tick: u32,
    pub fee_rate: u32,
    pub tick_spacing: Option<i32>,
    pub tick_map: HashMap<i32, TickInfo>,
    pub token_pair: TokenPair,
}

/// Information about a specific tick in the CLMM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickInfo {
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

/// Market update to be sent to the detector
#[derive(Debug, Clone)]
pub struct MarketUpdate {
    pub pool_address: String,
    pub dex_name: String,
    pub token_pair: TokenPair,
    pub sqrt_price: u128,
    pub liquidity: u128,
    pub tick: u32,
    pub fee_bps: u32,
    pub tick_map: HashMap<i32, TickInfo>,
}

/// Tick structure from event data
#[derive(Debug, Deserialize)]
pub struct TickData {
    pub bits: u32,
}

/// Pool snapshot event structure (common across CLMM DEXs)
#[derive(Debug, Deserialize)]
pub struct PoolSnapshotEvent {
    #[serde(rename = "pool_id")]
    pub pool_address: String,
    pub sqrt_price: String,
    pub liquidity: String,
    pub tick: TickData,
    pub fee_rate: String,
    pub tick_spacing: i32,
}

/// Swap after event structure (common across CLMM DEXs)
#[derive(Debug, Deserialize)]
pub struct SwapAfterEvent {
    #[serde(rename = "pool_id")]
    pub pool_address: String,
    #[serde(rename = "sqrt_price")]
    pub sqrt_price_after: String,
    #[serde(rename = "liquidity")]
    pub liquidity_after: String,
    #[serde(rename = "tick")]
    pub tick_after: TickData,
}
