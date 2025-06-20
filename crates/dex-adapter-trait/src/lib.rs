use anyhow::Result;
use async_trait::async_trait;
use common::types::{MarketUpdate, Transaction};

#[async_trait]
pub trait DexAdapter: Send + Sync {
    /// Returns the unique identifier for the adapter.
    fn id(&self) -> &'static str;

    /// Parses a transaction and returns a `MarketUpdate` if the transaction is relevant to the DEX.
    fn parse_transaction(&self, txn: &Transaction) -> Result<Option<MarketUpdate>>;
}
