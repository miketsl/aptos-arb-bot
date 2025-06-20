use anyhow::Result;
use common::types::{Event, MarketUpdate, Transaction};
use dex_adapter_trait::DexAdapter;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Parser {
    adapters: HashMap<String, Arc<dyn DexAdapter>>,
}

impl Parser {
    pub fn new(adapters: HashMap<String, Arc<dyn DexAdapter>>) -> Self {
        Self { adapters }
    }

    pub fn process_events(&self, events: &[Event], txn: &Transaction) -> Result<Vec<MarketUpdate>> {
        let mut updates = Vec::new();
        for event in events {
            if let Some(adapter) = self.adapters.get(&event.type_str) {
                if let Some(update) = adapter.parse_transaction(txn)? {
                    updates.push(update);
                }
            }
        }
        Ok(updates)
    }
}
