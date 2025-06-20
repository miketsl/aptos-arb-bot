use anyhow::Result;
use async_trait::async_trait;
use common::types::{MarketUpdate, Transaction};
use dex_adapter_trait::DexAdapter;

pub struct HyperionAdapter;

#[async_trait]
impl DexAdapter for HyperionAdapter {
    fn id(&self) -> &'static str {
        "hyperion"
    }

    fn parse_transaction(&self, _txn: &Transaction) -> Result<Option<MarketUpdate>> {
        Ok(None)
    }
}

pub struct ThalaAdapter;

#[async_trait]
impl DexAdapter for ThalaAdapter {
    fn id(&self) -> &'static str {
        "thala"
    }

    fn parse_transaction(&self, _txn: &Transaction) -> Result<Option<MarketUpdate>> {
        Ok(None)
    }
}

pub struct TappAdapter;

#[async_trait]
impl DexAdapter for TappAdapter {
    fn id(&self) -> &'static str {
        "tapp"
    }

    fn parse_transaction(&self, _txn: &Transaction) -> Result<Option<MarketUpdate>> {
        Ok(None)
    }
}
