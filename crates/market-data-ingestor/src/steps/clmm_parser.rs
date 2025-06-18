use crate::types::{
    DexConfig, MarketUpdate, PoolSnapshotEvent, PoolState, SwapAfterEvent, TokenPair,
};
use anyhow::{anyhow, Result};
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Event;
use std::collections::HashMap;
use tracing::{debug, error, warn};

/// Step that parses CLMM events and maintains pool state
pub struct ClmmParserStep {
    dex_configs: Vec<DexConfig>,
    pool_states: HashMap<String, PoolState>,
}

impl ClmmParserStep {
    pub fn new(dex_configs: Vec<DexConfig>) -> Self {
        Self {
            dex_configs,
            pool_states: HashMap::new(),
        }
    }

    /// Process events and generate market updates
    pub async fn process_events(&mut self, events: Vec<Event>) -> Result<Vec<MarketUpdate>> {
        let mut updates = Vec::new();

        for event in events {
            match self.process_single_event(event).await {
                Ok(Some(update)) => updates.push(update),
                Ok(None) => {}
                Err(e) => {
                    error!(error = %e, "Failed to process event");
                }
            }
        }

        Ok(updates)
    }

    /// Process a single event
    async fn process_single_event(&mut self, event: Event) -> Result<Option<MarketUpdate>> {
        let event_type = &event.type_str;

        // Find which DEX this event belongs to
        let dex = self
            .dex_configs
            .iter()
            .find(|d| {
                event_type == &d.pool_snapshot_event_name || event_type == &d.swap_event_name
            })
            .cloned()
            .ok_or_else(|| anyhow!("Unknown event type: {}", event_type))?;

        // Parse event data based on type
        if event_type == &dex.pool_snapshot_event_name {
            // Handle pool snapshot - this serves as both initialization and reconciliation
            let snapshot: PoolSnapshotEvent = serde_json::from_str(&event.data)
                .map_err(|e| anyhow!("Failed to parse PoolSnapshot: {}", e))?;
            
            let pool_address = snapshot.pool_address.clone();
            self.update_pool_from_snapshot(&dex, snapshot)?;
            
            // Generate update from the new state
            if let Some(pool_state) = self.pool_states.get(&pool_address) {
                return Ok(Some(self.create_market_update(pool_state)));
            }
        } else if event_type == &dex.swap_event_name {
            // Handle swap event
            let swap: SwapAfterEvent = serde_json::from_str(&event.data)
                .map_err(|e| anyhow!("Failed to parse SwapAfterEvent: {}", e))?;
            
            // Update pool state
            if let Some(pool_state) = self.pool_states.get_mut(&swap.pool_address) {
                pool_state.sqrt_price = swap
                    .sqrt_price_after
                    .parse()
                    .map_err(|e| anyhow!("Failed to parse sqrt_price_after: {}", e))?;
                pool_state.liquidity = swap
                    .liquidity_after
                    .parse()
                    .map_err(|e| anyhow!("Failed to parse liquidity_after: {}", e))?;
                pool_state.tick = swap.tick_after.bits;

                let update = MarketUpdate {
                    pool_address: pool_state.pool_address.clone(),
                    dex_name: pool_state.dex_name.clone(),
                    token_pair: pool_state.token_pair.clone(),
                    sqrt_price: pool_state.sqrt_price,
                    liquidity: pool_state.liquidity,
                    tick: pool_state.tick,
                    fee_bps: pool_state.fee_rate,
                    tick_map: pool_state.tick_map.clone(),
                };
                return Ok(Some(update));
            } else {
                // If we don't have state for this pool yet, we'll wait for a PoolSnapshot event
                warn!(
                    pool = swap.pool_address,
                    "Received swap event for pool without snapshot - ignoring until snapshot received"
                );
            }
        }

        Ok(None)
    }

    /// Update pool state from a snapshot event
    fn update_pool_from_snapshot(
        &mut self,
        dex: &DexConfig,
        snapshot: PoolSnapshotEvent,
    ) -> Result<()> {
        // Clone the pool address up front
        let pool_address = snapshot.pool_address.clone();

        let sqrt_price = snapshot
            .sqrt_price
            .parse::<u128>()
            .map_err(|e| anyhow!("Failed to parse sqrt_price: {}", e))?;
        let liquidity = snapshot
            .liquidity
            .parse::<u128>()
            .map_err(|e| anyhow!("Failed to parse liquidity: {}", e))?;
        let fee_rate = snapshot
            .fee_rate
            .parse::<u32>()
            .map_err(|e| anyhow!("Failed to parse fee_rate: {}", e))?;

        // TODO: Parse token pair from pool address or event data
        let token_pair = TokenPair {
            token0: "APT".to_string(),
            token1: "USDC".to_string(),
        };

        let pool_state = PoolState {
            pool_address: pool_address.clone(),
            dex_name: dex.name.clone(),
            sqrt_price,
            liquidity,
            tick: snapshot.tick.bits,
            fee_rate,
            tick_spacing: Some(snapshot.tick_spacing),
            tick_map: HashMap::new(), // TODO: Build tick map from events
            token_pair,
        };

        self.pool_states.insert(pool_address.clone(), pool_state);
        
        debug!(
            dex = dex.name,
            pool = %pool_address,
            "Pool state initialized/updated from snapshot"
        );
        
        Ok(())
    }

    /// Create a MarketUpdate from pool state
    fn create_market_update(&self, pool_state: &PoolState) -> MarketUpdate {
        MarketUpdate {
            pool_address: pool_state.pool_address.clone(),
            dex_name: pool_state.dex_name.clone(),
            token_pair: pool_state.token_pair.clone(),
            sqrt_price: pool_state.sqrt_price,
            liquidity: pool_state.liquidity,
            tick: pool_state.tick,
            fee_bps: pool_state.fee_rate,
            tick_map: pool_state.tick_map.clone(),
        }
    }
}
