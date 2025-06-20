use anyhow::Result;
use async_trait::async_trait;
use common::types::{Event, MarketUpdate};

#[async_trait]
pub trait DexAdapter: Send + Sync {
    /// Returns the unique identifier for the adapter.
    fn id(&self) -> &'static str;

    /// Parses an event and returns a `MarketUpdate` if the event is relevant to the DEX.
    fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>>;
}
