// Pool state manager - handles pool discovery and state fetching
use anyhow::Result;
use dex_adapters::{EventRouter, PoolState};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing;

/// Data source type determines pool discovery behavior
#[derive(Debug, Clone, PartialEq)]
pub enum DataSourceType {
    /// Live gRPC stream - fetch pool state via REST APIs
    Live,
    /// File replay - use embedded pool state data
    Replay,
}

/// Pool ID type for type safety
pub type PoolId = String;

/// Cached pool state with TTL
#[derive(Debug, Clone)]
pub struct CachedPoolState {
    pub state: PoolState,
    pub cached_at: Instant,
    pub ttl: Duration,
}

impl CachedPoolState {
    pub fn new(state: PoolState, ttl: Duration) -> Self {
        Self {
            state,
            cached_at: Instant::now(),
            ttl,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}

/// Configuration for pool filtering
#[derive(Debug, Clone)]
pub struct PoolFilterConfig {
    /// Minimum TVL threshold for tracking pools
    pub min_tvl_usd: Option<f64>,
    /// Token whitelist - only track pools containing these tokens
    pub token_whitelist: Option<Vec<String>>,
    /// Token blacklist - never track pools containing these tokens
    pub token_blacklist: Option<Vec<String>>,
    /// DEX whitelist - only track pools from these DEXes
    pub dex_whitelist: Option<Vec<String>>,
    /// Pool type whitelist - only track these pool types
    pub pool_type_whitelist: Option<Vec<String>>,
    /// Maximum number of pools to track simultaneously
    pub max_tracked_pools: Option<usize>,
}

impl Default for PoolFilterConfig {
    fn default() -> Self {
        Self {
            min_tvl_usd: Some(1000.0), // Default minimum $1000 TVL
            token_whitelist: None,
            token_blacklist: None,
            dex_whitelist: None,
            pool_type_whitelist: None,
            max_tracked_pools: Some(10000), // Reasonable default limit
        }
    }
}

/// Statistics for pool state management
#[derive(Debug, Clone, Default)]
pub struct PoolStateStats {
    pub pools_discovered: u64,
    pub pools_accepted: u64,
    pub pools_rejected: u64,
    pub pools_pending: u64,
    pub api_calls_made: u64,
    pub api_calls_successful: u64,
    pub api_calls_failed: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_evictions: u64,
}

/// Complete pool state management system
pub struct PoolStateManager {
    /// Pools we're actively tracking
    known_pools: Arc<RwLock<HashSet<PoolId>>>,
    /// Pools we've decided not to track (filtered out)
    rejected_pools: Arc<RwLock<HashSet<PoolId>>>,
    /// Pools currently being fetched asynchronously
    pending_pools: Arc<RwLock<HashSet<PoolId>>>,
    /// Completed async fetches waiting to be processed
    new_pool_state_cache: Arc<RwLock<HashMap<PoolId, CachedPoolState>>>,
    /// Event router for DEX adapter access
    event_router: Arc<EventRouter>,
    /// Data source type (live vs replay)
    #[allow(dead_code)]
    data_source_type: DataSourceType,
    /// Pool filtering configuration
    filter_config: PoolFilterConfig,
    /// Statistics tracking
    stats: Arc<RwLock<PoolStateStats>>,
    /// Channel for sending async worker results
    worker_result_tx: mpsc::UnboundedSender<WorkerResult>,
    /// Channel for receiving async worker results
    worker_result_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<WorkerResult>>>>,
    /// Default TTL for cached pool states
    default_cache_ttl: Duration,
}

/// Result from async pool state fetching worker
#[derive(Debug)]
pub struct WorkerResult {
    pub pool_id: PoolId,
    pub dex_name: String,
    pub result: Result<PoolState>,
    pub fetch_duration: Duration,
}

/// Pool discovery information
#[derive(Debug, Clone)]
pub struct PoolDiscovery {
    pub pool_id: PoolId,
    pub dex_name: String,
    pub discovered_in_event: String,
}

impl PoolStateManager {
    /// Create a new pool state manager
    pub fn new(
        event_router: Arc<EventRouter>,
        data_source_type: DataSourceType,
        filter_config: PoolFilterConfig,
    ) -> Self {
        let (worker_result_tx, worker_result_rx) = mpsc::unbounded_channel();

        Self {
            known_pools: Arc::new(RwLock::new(HashSet::new())),
            rejected_pools: Arc::new(RwLock::new(HashSet::new())),
            pending_pools: Arc::new(RwLock::new(HashSet::new())),
            new_pool_state_cache: Arc::new(RwLock::new(HashMap::new())),
            event_router,
            data_source_type,
            filter_config,
            stats: Arc::new(RwLock::new(PoolStateStats::default())),
            worker_result_tx,
            worker_result_rx: Arc::new(RwLock::new(Some(worker_result_rx))),
            default_cache_ttl: Duration::from_secs(300), // 5 minutes default
        }
    }

    /// Check if a pool is in any registry
    pub async fn check_pool_status(&self, pool_id: &PoolId) -> PoolStatus {
        // Check known pools first (most common case)
        if self.known_pools.read().await.contains(pool_id) {
            return PoolStatus::Known;
        }

        // Check rejected pools
        if self.rejected_pools.read().await.contains(pool_id) {
            return PoolStatus::Rejected;
        }

        // Check pending pools
        if self.pending_pools.read().await.contains(pool_id) {
            return PoolStatus::Pending;
        }

        PoolStatus::Unknown
    }

    /// Add a pool to the known pools registry
    pub async fn add_known_pool(&self, pool_id: PoolId) {
        let mut known_pools = self.known_pools.write().await;
        known_pools.insert(pool_id);
    }

    /// Add a pool to the rejected pools registry
    pub async fn add_rejected_pool(&self, pool_id: PoolId) {
        let mut rejected_pools = self.rejected_pools.write().await;
        rejected_pools.insert(pool_id);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.pools_rejected += 1;
    }

    /// Add a pool to the pending pools registry
    pub async fn add_pending_pool(&self, pool_id: PoolId) {
        let mut pending_pools = self.pending_pools.write().await;
        pending_pools.insert(pool_id);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.pools_pending += 1;
    }

    /// Remove a pool from the pending pools registry
    pub async fn remove_pending_pool(&self, pool_id: &PoolId) {
        let mut pending_pools = self.pending_pools.write().await;
        pending_pools.remove(pool_id);
    }

    /// Add a completed pool state to the cache
    pub async fn cache_pool_state(&self, pool_id: PoolId, pool_state: PoolState) {
        let cached_state = CachedPoolState::new(pool_state, self.default_cache_ttl);
        let mut cache = self.new_pool_state_cache.write().await;
        cache.insert(pool_id, cached_state);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.cache_misses += 1; // This was a new fetch
    }

    /// Get all completed pool states from cache and clear them
    pub async fn drain_cached_pool_states(&self) -> HashMap<PoolId, PoolState> {
        let mut cache = self.new_pool_state_cache.write().await;
        let mut result = HashMap::new();

        // Move non-expired entries to result
        let mut expired_keys = Vec::new();
        for (pool_id, cached_state) in cache.iter() {
            if cached_state.is_expired() {
                expired_keys.push(pool_id.clone());
            } else {
                result.insert(pool_id.clone(), cached_state.state.clone());
            }
        }

        // Remove expired entries
        for key in expired_keys {
            cache.remove(&key);
            let mut stats = self.stats.write().await;
            stats.cache_evictions += 1;
        }

        // Clear processed entries
        for pool_id in result.keys() {
            cache.remove(pool_id);
        }

        result
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> PoolStateStats {
        self.stats.read().await.clone()
    }

    /// Get registry sizes for monitoring
    pub async fn get_registry_sizes(&self) -> RegistrySizes {
        RegistrySizes {
            known_pools: self.known_pools.read().await.len(),
            rejected_pools: self.rejected_pools.read().await.len(),
            pending_pools: self.pending_pools.read().await.len(),
            cached_states: self.new_pool_state_cache.read().await.len(),
        }
    }

    /// Clean up expired cache entries
    pub async fn cleanup_expired_cache(&self) {
        let mut cache = self.new_pool_state_cache.write().await;
        let mut expired_keys = Vec::new();

        for (pool_id, cached_state) in cache.iter() {
            if cached_state.is_expired() {
                expired_keys.push(pool_id.clone());
            }
        }

        let mut stats = self.stats.write().await;
        for key in expired_keys {
            cache.remove(&key);
            stats.cache_evictions += 1;
        }

        if stats.cache_evictions > 0 {
            tracing::debug!(
                evicted = stats.cache_evictions,
                "Cleaned up expired pool state cache entries"
            );
        }
    }

    /// Extract pool IDs from a transaction using the event router
    pub async fn extract_pool_ids_from_transaction(
        &self,
        transaction: &aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction,
    ) -> Result<Vec<PoolDiscovery>> {
        let mut discoveries = Vec::new();

        // For now, we'll implement a simplified version that works with the current transaction structure
        // In a real implementation, you'd need to properly extract events from the transaction

        // This is a placeholder implementation for demonstration
        // Real implementation would parse transaction events and extract actual pool references

        // For testing purposes, occasionally "discover" a mock pool
        if transaction.version % 100 == 0 {
            let mock_discovery = PoolDiscovery {
                pool_id: format!("pool_from_txn_{}", transaction.version),
                dex_name: "hyperion".to_string(), // Would be determined from actual transaction data
                discovered_in_event: "mock_event".to_string(),
            };

            discoveries.push(mock_discovery);
        }

        // Update discovery stats
        let mut stats = self.stats.write().await;
        stats.pools_discovered += discoveries.len() as u64;

        Ok(discoveries)
    }

    /// Determine DEX name from transaction data (simplified implementation)
    #[allow(dead_code)]
    fn determine_dex_from_transaction(
        &self,
        _transaction: &aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction,
    ) -> Result<String> {
        // This is a simplified implementation
        // In practice, you'd parse the transaction payload to determine the DEX

        // For now, default to hyperion
        // Real implementation would check transaction payload, function calls, etc.
        Ok("hyperion".to_string())
    }

    /// Process discovered pools and determine which ones to track
    pub async fn process_pool_discoveries(
        &self,
        discoveries: Vec<PoolDiscovery>,
    ) -> Vec<PoolDiscovery> {
        let mut pools_to_fetch = Vec::new();
        let mut pools_added_in_batch = 0;

        for discovery in discoveries {
            // Check if pool is already in any registry
            let status = self.check_pool_status(&discovery.pool_id).await;

            match status {
                PoolStatus::Known | PoolStatus::Rejected | PoolStatus::Pending => {
                    // Skip pools we already know about
                    continue;
                }
                PoolStatus::Unknown => {
                    // Apply filtering logic with current batch count
                    if self
                        .should_track_pool_with_batch_count(&discovery, pools_added_in_batch)
                        .await
                    {
                        pools_to_fetch.push(discovery);
                        pools_added_in_batch += 1;
                    } else {
                        // Add to rejected pools
                        let pool_id = discovery.pool_id.clone();
                        let dex_name = discovery.dex_name.clone();
                        self.add_rejected_pool(discovery.pool_id).await;
                        tracing::debug!(
                            pool_id = %pool_id,
                            dex = %dex_name,
                            "Pool rejected by filters"
                        );
                    }
                }
            }
        }

        pools_to_fetch
    }

    /// Apply filtering logic to determine if a pool should be tracked
    #[allow(dead_code)]
    async fn should_track_pool(&self, discovery: &PoolDiscovery) -> bool {
        self.should_track_pool_with_batch_count(discovery, 0).await
    }

    /// Apply filtering logic with batch count to determine if a pool should be tracked
    async fn should_track_pool_with_batch_count(
        &self,
        discovery: &PoolDiscovery,
        pools_added_in_batch: usize,
    ) -> bool {
        // Basic validation - reject empty/invalid IDs
        if discovery.pool_id.is_empty() || discovery.dex_name.is_empty() {
            tracing::debug!(
                pool_id = %discovery.pool_id,
                dex = %discovery.dex_name,
                "Pool rejected: invalid empty pool ID or DEX name"
            );
            return false;
        }

        // Validate that the DEX is known to the event router
        if !self.event_router.is_dex_supported(&discovery.dex_name) {
            tracing::debug!(
                pool_id = %discovery.pool_id,
                dex = %discovery.dex_name,
                "Pool rejected: unknown/unsupported DEX"
            );
            return false;
        }

        // Check DEX whitelist
        if let Some(ref dex_whitelist) = self.filter_config.dex_whitelist {
            if !dex_whitelist.contains(&discovery.dex_name) {
                tracing::debug!(
                    pool_id = %discovery.pool_id,
                    dex = %discovery.dex_name,
                    "Pool rejected: DEX not in whitelist"
                );
                return false;
            }
        }

        // Check if we've reached the maximum number of tracked pools
        if let Some(max_pools) = self.filter_config.max_tracked_pools {
            let known_count = self.known_pools.read().await.len();
            let pending_count = self.pending_pools.read().await.len();

            if known_count + pending_count + pools_added_in_batch >= max_pools {
                tracing::debug!(
                    pool_id = %discovery.pool_id,
                    known_pools = known_count,
                    pending_pools = pending_count,
                    batch_pools = pools_added_in_batch,
                    max_pools = max_pools,
                    "Pool rejected: maximum tracked pools limit reached"
                );
                return false;
            }
        }

        // For now, accept pools that pass basic filters
        // More sophisticated filtering (TVL, token filters) would require
        // fetching pool state first, which we'll do in the async worker
        true
    }

    /// Apply advanced filtering that requires pool state data
    pub fn should_track_pool_with_state(&self, pool_state: &PoolState) -> bool {
        // Check token whitelist
        if let Some(ref token_whitelist) = self.filter_config.token_whitelist {
            let has_whitelisted_token = token_whitelist.contains(&pool_state.token_a)
                || token_whitelist.contains(&pool_state.token_b);

            if !has_whitelisted_token {
                tracing::debug!(
                    pool_id = %pool_state.pool_id,
                    token_a = %pool_state.token_a,
                    token_b = %pool_state.token_b,
                    "Pool rejected: tokens not in whitelist"
                );
                return false;
            }
        }

        // Check token blacklist
        if let Some(ref token_blacklist) = self.filter_config.token_blacklist {
            let has_blacklisted_token = token_blacklist.contains(&pool_state.token_a)
                || token_blacklist.contains(&pool_state.token_b);

            if has_blacklisted_token {
                tracing::debug!(
                    pool_id = %pool_state.pool_id,
                    token_a = %pool_state.token_a,
                    token_b = %pool_state.token_b,
                    "Pool rejected: tokens in blacklist"
                );
                return false;
            }
        }

        // Check pool type whitelist
        if let Some(ref pool_type_whitelist) = self.filter_config.pool_type_whitelist {
            // Extract pool type from additional_data
            if let Some(pool_type) = pool_state.additional_data.get("pool_type") {
                if let Some(pool_type_str) = pool_type.as_str() {
                    if !pool_type_whitelist.contains(&pool_type_str.to_string()) {
                        tracing::debug!(
                            pool_id = %pool_state.pool_id,
                            pool_type = %pool_type_str,
                            "Pool rejected: pool type not in whitelist"
                        );
                        return false;
                    }
                }
            }
        }

        // Check minimum TVL (disabled - requires price feed integration)
        if let Some(_min_tvl_usd) = self.filter_config.min_tvl_usd {
            // TODO: Implement proper TVL calculation with token prices
            // For now, this filter is disabled since we don't have price feeds
            tracing::warn!("USD TVL filtering is configured but not implemented - requires price feed integration");
        }

        true
    }

    /// Get the worker result receiver (should only be called once)
    pub async fn take_worker_result_receiver(
        &self,
    ) -> Option<mpsc::UnboundedReceiver<WorkerResult>> {
        let mut rx_option = self.worker_result_rx.write().await;
        rx_option.take()
    }

    /// Get the worker result sender for spawning async workers
    pub fn get_worker_result_sender(&self) -> mpsc::UnboundedSender<WorkerResult> {
        self.worker_result_tx.clone()
    }

    /// Spawn async workers to fetch pool states for discovered pools
    pub async fn spawn_pool_state_workers(&self, discoveries: Vec<PoolDiscovery>) {
        for discovery in discoveries {
            // Add to pending pools to prevent duplicate fetches
            self.add_pending_pool(discovery.pool_id.clone()).await;

            // Spawn async worker
            let event_router = self.event_router.clone();
            let worker_tx = self.worker_result_tx.clone();
            let pool_id = discovery.pool_id.clone();
            let dex_name = discovery.dex_name.clone();

            tokio::spawn(async move {
                let start_time = Instant::now();

                tracing::debug!(
                    pool_id = %pool_id,
                    dex = %dex_name,
                    "Starting async pool state fetch"
                );

                // Fetch pool state using the appropriate DEX adapter
                let result = event_router.fetch_pool_state(&pool_id, &dex_name).await;

                let fetch_duration = start_time.elapsed();

                match &result {
                    Ok(pool_state) => {
                        tracing::info!(
                            pool_id = %pool_id,
                            dex = %dex_name,
                            duration_ms = fetch_duration.as_millis(),
                            token_a = %pool_state.token_a,
                            token_b = %pool_state.token_b,
                            "Successfully fetched pool state"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            pool_id = %pool_id,
                            dex = %dex_name,
                            duration_ms = fetch_duration.as_millis(),
                            error = %e,
                            "Failed to fetch pool state"
                        );
                    }
                }

                // Send result back to main thread
                let worker_result = WorkerResult {
                    pool_id,
                    dex_name,
                    result,
                    fetch_duration,
                };

                if let Err(e) = worker_tx.send(worker_result) {
                    tracing::error!(error = %e, "Failed to send worker result");
                }
            });
        }
    }

    /// Process completed worker results
    pub async fn process_worker_results(&self, results: Vec<WorkerResult>) {
        let mut stats = self.stats.write().await;

        for result in results {
            // Remove from pending pools
            self.remove_pending_pool(&result.pool_id).await;

            // Update API call stats
            stats.api_calls_made += 1;

            match result.result {
                Ok(pool_state) => {
                    stats.api_calls_successful += 1;

                    // Apply advanced filtering that requires pool state
                    if self.should_track_pool_with_state(&pool_state) {
                        // Cache the pool state
                        self.cache_pool_state(result.pool_id.clone(), pool_state)
                            .await;
                        stats.pools_accepted += 1;

                        tracing::info!(
                            pool_id = %result.pool_id,
                            dex = %result.dex_name,
                            duration_ms = result.fetch_duration.as_millis(),
                            "Pool accepted and cached"
                        );
                    } else {
                        // Pool was rejected by advanced filters
                        self.add_rejected_pool(result.pool_id.clone()).await;

                        tracing::debug!(
                            pool_id = %result.pool_id,
                            dex = %result.dex_name,
                            "Pool rejected by advanced filters"
                        );
                    }
                }
                Err(e) => {
                    stats.api_calls_failed += 1;

                    // For now, add failed fetches to rejected pools
                    // In production, you might want to retry or handle differently
                    self.add_rejected_pool(result.pool_id.clone()).await;

                    tracing::warn!(
                        pool_id = %result.pool_id,
                        dex = %result.dex_name,
                        error = %e,
                        duration_ms = result.fetch_duration.as_millis(),
                        "Pool fetch failed, adding to rejected pools"
                    );
                }
            }
        }
    }

    /// Spawn a background task to process worker results
    pub async fn start_worker_result_processor(&self) -> tokio::task::JoinHandle<()> {
        let mut receiver = self
            .take_worker_result_receiver()
            .await
            .expect("Worker result receiver should only be taken once");

        let manager = PoolStateManagerHandle {
            known_pools: self.known_pools.clone(),
            rejected_pools: self.rejected_pools.clone(),
            pending_pools: self.pending_pools.clone(),
            new_pool_state_cache: self.new_pool_state_cache.clone(),
            stats: self.stats.clone(),
            filter_config: self.filter_config.clone(),
            default_cache_ttl: self.default_cache_ttl,
        };

        tokio::spawn(async move {
            let mut results_batch = Vec::new();
            let mut batch_timer = tokio::time::interval(Duration::from_millis(100));

            loop {
                tokio::select! {
                    // Collect worker results
                    result = receiver.recv() => {
                        match result {
                            Some(worker_result) => {
                                results_batch.push(worker_result);

                                // Process batch if it gets large
                                if results_batch.len() >= 10 {
                                    manager.process_worker_results_batch(&mut results_batch).await;
                                }
                            }
                            None => {
                                // Channel closed, process remaining results and exit
                                if !results_batch.is_empty() {
                                    manager.process_worker_results_batch(&mut results_batch).await;
                                }
                                break;
                            }
                        }
                    }

                    // Process batch periodically
                    _ = batch_timer.tick() => {
                        if !results_batch.is_empty() {
                            manager.process_worker_results_batch(&mut results_batch).await;
                        }
                    }
                }
            }

            tracing::info!("Worker result processor task completed");
        })
    }
}

/// Pool status in registries
#[derive(Debug, Clone, PartialEq)]
pub enum PoolStatus {
    /// Pool is in known_pools registry
    Known,
    /// Pool is in rejected_pools registry
    Rejected,
    /// Pool is in pending_pools registry (being fetched)
    Pending,
    /// Pool is not in any registry
    Unknown,
}

/// Registry sizes for monitoring
#[derive(Debug, Clone)]
pub struct RegistrySizes {
    pub known_pools: usize,
    pub rejected_pools: usize,
    pub pending_pools: usize,
    pub cached_states: usize,
}

/// Handle for processing worker results in background task
#[derive(Clone)]
struct PoolStateManagerHandle {
    #[allow(dead_code)]
    known_pools: Arc<RwLock<HashSet<PoolId>>>,
    rejected_pools: Arc<RwLock<HashSet<PoolId>>>,
    pending_pools: Arc<RwLock<HashSet<PoolId>>>,
    new_pool_state_cache: Arc<RwLock<HashMap<PoolId, CachedPoolState>>>,
    stats: Arc<RwLock<PoolStateStats>>,
    filter_config: PoolFilterConfig,
    default_cache_ttl: Duration,
}

impl PoolStateManagerHandle {
    /// Process a batch of worker results
    async fn process_worker_results_batch(&self, results: &mut Vec<WorkerResult>) {
        let mut stats = self.stats.write().await;

        for result in results.drain(..) {
            // Remove from pending pools
            {
                let mut pending_pools = self.pending_pools.write().await;
                pending_pools.remove(&result.pool_id);
            }

            // Update API call stats
            stats.api_calls_made += 1;

            match result.result {
                Ok(pool_state) => {
                    stats.api_calls_successful += 1;

                    // Apply advanced filtering that requires pool state
                    if self.should_track_pool_with_state(&pool_state) {
                        // Cache the pool state
                        let cached_state = CachedPoolState::new(pool_state, self.default_cache_ttl);
                        {
                            let mut cache = self.new_pool_state_cache.write().await;
                            cache.insert(result.pool_id.clone(), cached_state);
                        }
                        stats.pools_accepted += 1;

                        tracing::info!(
                            pool_id = %result.pool_id,
                            dex = %result.dex_name,
                            duration_ms = result.fetch_duration.as_millis(),
                            "Pool accepted and cached"
                        );
                    } else {
                        // Pool was rejected by advanced filters
                        {
                            let mut rejected_pools = self.rejected_pools.write().await;
                            rejected_pools.insert(result.pool_id.clone());
                        }
                        stats.pools_rejected += 1;

                        tracing::debug!(
                            pool_id = %result.pool_id,
                            dex = %result.dex_name,
                            "Pool rejected by advanced filters"
                        );
                    }
                }
                Err(e) => {
                    stats.api_calls_failed += 1;

                    // Add failed fetches to rejected pools
                    {
                        let mut rejected_pools = self.rejected_pools.write().await;
                        rejected_pools.insert(result.pool_id.clone());
                    }
                    stats.pools_rejected += 1;

                    tracing::warn!(
                        pool_id = %result.pool_id,
                        dex = %result.dex_name,
                        error = %e,
                        duration_ms = result.fetch_duration.as_millis(),
                        "Pool fetch failed, adding to rejected pools"
                    );
                }
            }
        }
    }

    /// Apply advanced filtering that requires pool state data
    fn should_track_pool_with_state(&self, pool_state: &PoolState) -> bool {
        // Check token whitelist
        if let Some(ref token_whitelist) = self.filter_config.token_whitelist {
            let has_whitelisted_token = token_whitelist.contains(&pool_state.token_a)
                || token_whitelist.contains(&pool_state.token_b);

            if !has_whitelisted_token {
                return false;
            }
        }

        // Check token blacklist
        if let Some(ref token_blacklist) = self.filter_config.token_blacklist {
            let has_blacklisted_token = token_blacklist.contains(&pool_state.token_a)
                || token_blacklist.contains(&pool_state.token_b);

            if has_blacklisted_token {
                return false;
            }
        }

        // Check pool type whitelist
        if let Some(ref pool_type_whitelist) = self.filter_config.pool_type_whitelist {
            if let Some(pool_type) = pool_state.additional_data.get("pool_type") {
                if let Some(pool_type_str) = pool_type.as_str() {
                    if !pool_type_whitelist.contains(&pool_type_str.to_string()) {
                        return false;
                    }
                }
            }
        }

        // Check minimum TVL (disabled - requires price feed integration)
        if let Some(_min_tvl_usd) = self.filter_config.min_tvl_usd {
            // TODO: Implement proper TVL calculation with token prices
            // For now, this filter is disabled since we don't have price feeds
            tracing::warn!("USD TVL filtering is configured but not implemented - requires price feed integration");
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dex_adapters::{EventRouter, HyperionAdapter};

    fn create_test_event_router() -> Arc<EventRouter> {
        let mut router = EventRouter::new();
        let hyperion = Arc::new(HyperionAdapter::default());
        router.register_adapter(hyperion);
        Arc::new(router)
    }

    #[tokio::test]
    async fn test_pool_state_manager_creation() {
        let event_router = create_test_event_router();
        let filter_config = PoolFilterConfig::default();

        let manager = PoolStateManager::new(event_router, DataSourceType::Live, filter_config);

        assert_eq!(manager.data_source_type, DataSourceType::Live);

        let sizes = manager.get_registry_sizes().await;
        assert_eq!(sizes.known_pools, 0);
        assert_eq!(sizes.rejected_pools, 0);
        assert_eq!(sizes.pending_pools, 0);
        assert_eq!(sizes.cached_states, 0);
    }

    #[tokio::test]
    async fn test_pool_status_checking() {
        let event_router = create_test_event_router();
        let filter_config = PoolFilterConfig::default();

        let manager = PoolStateManager::new(event_router, DataSourceType::Live, filter_config);

        let pool_id = "test_pool_123".to_string();

        // Initially unknown
        assert_eq!(
            manager.check_pool_status(&pool_id).await,
            PoolStatus::Unknown
        );

        // Add to known pools
        manager.add_known_pool(pool_id.clone()).await;
        assert_eq!(manager.check_pool_status(&pool_id).await, PoolStatus::Known);

        // Test with different pool for rejected
        let rejected_pool = "rejected_pool_456".to_string();
        manager.add_rejected_pool(rejected_pool.clone()).await;
        assert_eq!(
            manager.check_pool_status(&rejected_pool).await,
            PoolStatus::Rejected
        );

        // Test with different pool for pending
        let pending_pool = "pending_pool_789".to_string();
        manager.add_pending_pool(pending_pool.clone()).await;
        assert_eq!(
            manager.check_pool_status(&pending_pool).await,
            PoolStatus::Pending
        );
    }

    #[tokio::test]
    async fn test_cached_pool_state_ttl() {
        let pool_state = PoolState {
            pool_id: "test_pool".to_string(),
            dex_name: "hyperion".to_string(),
            token_a: "APT".to_string(),
            token_b: "USDC".to_string(),
            reserve_a: "1000000".to_string(),
            reserve_b: "500000".to_string(),
            fee_rate: "0.003".to_string(),
            block_height: 12345,
            additional_data: serde_json::json!({}),
        };

        // Test non-expired cache
        let cached = CachedPoolState::new(pool_state.clone(), Duration::from_secs(60));
        assert!(!cached.is_expired());

        // Test expired cache
        let cached_expired = CachedPoolState::new(pool_state, Duration::from_millis(1));
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert!(cached_expired.is_expired());
    }

    #[tokio::test]
    async fn test_pool_state_caching() {
        let event_router = create_test_event_router();
        let filter_config = PoolFilterConfig::default();

        let manager = PoolStateManager::new(event_router, DataSourceType::Live, filter_config);

        let pool_state = PoolState {
            pool_id: "test_pool".to_string(),
            dex_name: "hyperion".to_string(),
            token_a: "APT".to_string(),
            token_b: "USDC".to_string(),
            reserve_a: "1000000".to_string(),
            reserve_b: "500000".to_string(),
            fee_rate: "0.003".to_string(),
            block_height: 12345,
            additional_data: serde_json::json!({}),
        };

        // Cache a pool state
        manager
            .cache_pool_state("test_pool".to_string(), pool_state.clone())
            .await;

        let sizes = manager.get_registry_sizes().await;
        assert_eq!(sizes.cached_states, 1);

        // Drain cached states
        let drained = manager.drain_cached_pool_states().await;
        assert_eq!(drained.len(), 1);
        assert!(drained.contains_key("test_pool"));

        // Cache should be empty after draining
        let sizes = manager.get_registry_sizes().await;
        assert_eq!(sizes.cached_states, 0);
    }

    #[test]
    fn test_pool_filter_config_default() {
        let config = PoolFilterConfig::default();
        assert_eq!(config.min_tvl_usd, Some(1000.0));
        assert_eq!(config.max_tracked_pools, Some(10000));
        assert!(config.token_whitelist.is_none());
        assert!(config.token_blacklist.is_none());
    }
}
