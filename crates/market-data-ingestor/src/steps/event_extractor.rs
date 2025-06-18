use crate::types::DexConfig;
use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::{Event, Transaction};
use tracing::{info, trace};

/// Step that filters transactions for relevant DEX events
pub struct EventExtractorStep {
    dex_configs: Vec<DexConfig>,
}

impl EventExtractorStep {
    pub fn new(dex_configs: Vec<DexConfig>) -> Self {
        Self { dex_configs }
    }

    fn is_relevant_event(&self, event: &Event) -> bool {
        let type_str = &event.type_str;
        let event_address = if let Some(key) = &event.key {
            &key.account_address
        } else {
            return false;
        };

        trace!(checking_event_type = type_str, address = event_address, "Checking event");

        if let Some(dex) = self.dex_configs.iter().find(|dex| {
            type_str == &dex.pool_snapshot_event_name || type_str == &dex.swap_event_name
        }) {
            // If a DEX is found, check if we need to filter by specific pools
            if dex.pools.is_empty() {
                info!(
                    event_type = type_str,
                    dex = dex.name,
                    pool_address = event_address,
                    "Found matching event (no pool filter)"
                );
                return true;
            }

            if dex.pools.contains(event_address) {
                info!(
                    event_type = type_str,
                    dex = dex.name,
                    pool_address = event_address,
                    "Found matching event (pool filter passed)"
                );
                return true;
            } else {
                // This is the case we want to debug
                trace!(
                    event_type = type_str,
                    dex = dex.name,
                    event_address = event_address,
                    configured_pools = ?dex.pools,
                    "Event type matched but pool address did not"
                );
            }
        }

        false
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
