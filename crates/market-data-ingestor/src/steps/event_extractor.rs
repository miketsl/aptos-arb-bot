use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::{Event, Transaction};
use config_lib::DexConfig;
use std::collections::HashSet;
use tracing::{info, trace};

/// Step that filters transactions for relevant DEX events
pub struct EventExtractorStep {
    relevant_event_types: HashSet<String>,
}

impl EventExtractorStep {
    pub fn new(dex_configs: Vec<DexConfig>) -> Self {
        // Pre-allocate and build full event-type keys once to avoid repeated re-allocations
        let total = dex_configs.iter().map(|d| d.events.len()).sum();
        let mut relevant_event_types = HashSet::with_capacity(total);
        for dex in dex_configs {
            for event_suffix in dex.events.values() {
                let mut key = String::with_capacity(dex.module_address.len() + event_suffix.len());
                key.push_str(&dex.module_address);
                key.push_str(event_suffix);
                relevant_event_types.insert(key);
            }
        }
        info!(
            "EventExtractor initialized with event types: {:?}",
            relevant_event_types
        );
        Self {
            relevant_event_types,
        }
    }

    fn is_relevant_event(&self, event: &Event) -> bool {
        self.relevant_event_types.contains(&event.type_str)
    }

    pub async fn process_transaction(&mut self, transaction: Transaction) -> Result<Vec<Event>> {
        let version = transaction.version;

        // Extract events from user transactions
        let mut relevant_events = Vec::new();

        if let Some(txn_data) = transaction.txn_data {
            use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::transaction::TxnData;

            if let TxnData::User(user_txn) = txn_data {
                for event in user_txn.events {
                    if self.is_relevant_event(&event) {
                        relevant_events.push(event);
                    }
                }
            }
        }

        if relevant_events.is_empty() {
            trace!(version = version, "No relevant events found in transaction");
        } else {
            info!(
                version = version,
                event_count = relevant_events.len(),
                "Found relevant events"
            );
        }

        Ok(relevant_events)
    }
}
