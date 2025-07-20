use anyhow::Result;
use async_trait::async_trait;
use common::types::{Event, MarketUpdate};

/// Pool state information for recording and replay
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolState {
    pub pool_id: String,
    pub dex_name: String,
    pub token_a: String,
    pub token_b: String,
    pub reserve_a: String, // Decimal as string for precision
    pub reserve_b: String, // Decimal as string for precision
    pub fee_rate: String,  // Decimal as string for precision
    pub block_height: u64,
    pub additional_data: serde_json::Value, // DEX-specific data
}

#[async_trait]
pub trait DexAdapter: Send + Sync {
    /// Returns the unique identifier for the adapter.
    fn id(&self) -> &'static str;

    /// Parses an event and returns a `MarketUpdate` if the event is relevant to the DEX.
    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>>;

    /// Returns the module addresses this adapter handles for event routing
    fn module_addresses(&self) -> &[String];

    /// Extracts pool IDs from an event if it contains pool references
    fn extract_pool_ids(&self, event: &Event) -> Result<Vec<String>>;

    /// Fetches current pool state from the DEX's REST API
    async fn fetch_pool_state(&self, pool_id: &str) -> Result<PoolState>;
}

mod event_router;
mod hyperion;
mod tapp;
mod thala;

pub use event_router::EventRouter;
pub use hyperion::HyperionAdapter;
pub use tapp::TappAdapter;
pub use thala::ThalaAdapter;

/// Create a DEX adapter from configuration
pub fn create_adapter_from_config(
    name: &str,
    module_address: String,
) -> Result<Box<dyn DexAdapter>> {
    let module_addresses = vec![module_address];

    match name.to_lowercase().as_str() {
        "hyperion" => Ok(Box::new(HyperionAdapter::new(module_addresses))),
        "thalaswap" | "thala" => Ok(Box::new(ThalaAdapter::new(module_addresses))),
        "tapp" => Ok(Box::new(TappAdapter::new(module_addresses))),
        _ => Err(anyhow::anyhow!("Unknown DEX adapter: {}", name)),
    }
}
