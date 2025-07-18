use crate::{DexAdapter, PoolState};
use anyhow::Result;
use common::types::{Event, MarketUpdate};
use std::collections::HashMap;
use std::sync::Arc;

/// Event router that routes events to appropriate DEX adapters based on module addresses
pub struct EventRouter {
    /// Map from module address to adapter
    adapters: HashMap<String, Arc<dyn DexAdapter>>,
    /// All registered adapters for iteration
    all_adapters: Vec<Arc<dyn DexAdapter>>,
}

impl EventRouter {
    /// Create a new event router
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
            all_adapters: Vec::new(),
        }
    }

    /// Register a DEX adapter with the router
    pub fn register_adapter(&mut self, adapter: Arc<dyn DexAdapter>) {
        // Register all module addresses for this adapter
        for module_address in adapter.module_addresses() {
            self.adapters.insert(module_address.clone(), adapter.clone());
        }
        
        // Keep track of all adapters
        if !self.all_adapters.iter().any(|a| a.id() == adapter.id()) {
            self.all_adapters.push(adapter);
        }
    }

    /// Route an event to the appropriate adapter and parse it
    pub fn route_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
        // Extract module address from event (this would need to be implemented based on event structure)
        let module_address = self.extract_module_address(event)?;
        
        // Find the appropriate adapter
        if let Some(adapter) = self.adapters.get(&module_address) {
            adapter.parse_event(event)
        } else {
            // No adapter registered for this module address
            Ok(None)
        }
    }

    /// Extract pool IDs from an event using all registered adapters
    pub fn extract_pool_ids(&self, event: &Event) -> Result<Vec<String>> {
        let mut all_pool_ids = Vec::new();
        
        for adapter in &self.all_adapters {
            let pool_ids = adapter.extract_pool_ids(event)?;
            all_pool_ids.extend(pool_ids);
        }
        
        // Remove duplicates
        all_pool_ids.sort();
        all_pool_ids.dedup();
        
        Ok(all_pool_ids)
    }

    /// Fetch pool state using the appropriate adapter
    pub async fn fetch_pool_state(&self, pool_id: &str, dex_name: &str) -> Result<PoolState> {
        // Find adapter by DEX name
        for adapter in &self.all_adapters {
            if adapter.id() == dex_name {
                return adapter.fetch_pool_state(pool_id).await;
            }
        }
        
        Err(anyhow::anyhow!("No adapter found for DEX: {}", dex_name))
    }

    /// Get all registered adapters
    pub fn get_adapters(&self) -> &[Arc<dyn DexAdapter>] {
        &self.all_adapters
    }

    /// Get adapter by DEX name
    pub fn get_adapter(&self, dex_name: &str) -> Option<Arc<dyn DexAdapter>> {
        self.all_adapters.iter()
            .find(|adapter| adapter.id() == dex_name)
            .cloned()
    }

    /// Extract module address from event
    /// This is a simplified implementation - real implementation would parse the event structure
    fn extract_module_address(&self, event: &Event) -> Result<String> {
        // This is a placeholder implementation
        // In practice, this would extract the module address from the event's type or metadata
        
        // For now, try to match against known patterns in event data
        let event_str = &event.data;
        
        // Check for known DEX patterns
        if event_str.contains("hyperion") {
            Ok("0x1::hyperion::pool".to_string())
        } else if event_str.contains("thala") {
            Ok("0x2::thala::pool".to_string())
        } else if event_str.contains("tapp") {
            Ok("0x3::tapp::pool".to_string())
        } else {
            // Default to first registered module if no match
            self.adapters.keys().next()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No adapters registered"))
        }
    }
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HyperionAdapter, ThalaAdapter, TappAdapter};
    use common::types::Event;

    #[test]
    fn test_event_router_creation() {
        let router = EventRouter::new();
        assert_eq!(router.all_adapters.len(), 0);
        assert_eq!(router.adapters.len(), 0);
    }

    #[test]
    fn test_register_adapters() {
        let mut router = EventRouter::new();
        
        let hyperion = Arc::new(HyperionAdapter::default());
        let thala = Arc::new(ThalaAdapter::default());
        let tapp = Arc::new(TappAdapter::default());
        
        router.register_adapter(hyperion.clone());
        router.register_adapter(thala.clone());
        router.register_adapter(tapp.clone());
        
        assert_eq!(router.all_adapters.len(), 3);
        assert!(router.adapters.len() > 0); // Should have module address mappings
    }

    #[test]
    fn test_get_adapter_by_name() {
        let mut router = EventRouter::new();
        
        let hyperion = Arc::new(HyperionAdapter::default());
        router.register_adapter(hyperion.clone());
        
        let found = router.get_adapter("hyperion");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id(), "hyperion");
        
        let not_found = router.get_adapter("nonexistent");
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_fetch_pool_state_routing() {
        let mut router = EventRouter::new();
        
        let hyperion = Arc::new(HyperionAdapter::default());
        router.register_adapter(hyperion.clone());
        
        // This would fail in practice due to network call, but tests the routing logic
        let result = router.fetch_pool_state("test_pool", "hyperion").await;
        assert!(result.is_err()); // Expected to fail due to network call
        
        // Test with unknown DEX
        let result = router.fetch_pool_state("test_pool", "unknown").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No adapter found"));
    }

    #[test]
    fn test_extract_module_address() {
        let mut router = EventRouter::new();
        
        // Register an adapter first
        let hyperion = Arc::new(HyperionAdapter::default());
        router.register_adapter(hyperion.clone());
        
        let hyperion_event = Event {
            data: r#"{"pool_id": "0x123", "dex": "hyperion"}"#.to_string(),
            ..Default::default()
        };
        
        let result = router.extract_module_address(&hyperion_event);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("hyperion"));
    }

    #[test]
    fn test_extract_pool_ids() {
        let mut router = EventRouter::new();
        
        let hyperion = Arc::new(HyperionAdapter::default());
        router.register_adapter(hyperion.clone());
        
        let event = Event {
            data: r#"{"pool_id": "test_pool_123"}"#.to_string(),
            ..Default::default()
        };
        
        let pool_ids = router.extract_pool_ids(&event).unwrap();
        // The actual result depends on the adapter implementation
        // Just verify we get a valid result (could be empty)
    }
}