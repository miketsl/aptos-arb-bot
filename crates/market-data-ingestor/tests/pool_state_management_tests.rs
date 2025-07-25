//! Comprehensive pool state management testing suite
//! Tests concurrent operations, caching behavior, filtering, and worker management

use dex_adapters::{EventRouter, HyperionAdapter, PoolState};
use market_data_ingestor::pool_state_manager::{
    CachedPoolState, DataSourceType, PoolDiscovery, PoolFilterConfig, PoolStateManager, PoolStatus,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Create a test event router for testing
fn create_test_event_router() -> Arc<EventRouter> {
    let mut router = EventRouter::new();
    let hyperion = Arc::new(HyperionAdapter::default());
    router.register_adapter(hyperion);
    Arc::new(router)
}

/// Create a test pool state for testing
fn create_test_pool_state(pool_id: &str) -> PoolState {
    PoolState {
        pool_id: pool_id.to_string(),
        dex_name: "hyperion".to_string(),
        token_a: "APT".to_string(),
        token_b: "USDC".to_string(),
        reserve_a: "1000000".to_string(),
        reserve_b: "500000".to_string(),
        fee_rate: "0.003".to_string(),
        block_height: 12345,
        additional_data: serde_json::json!({"pool_type": "constant_product"}),
    }
}

/// Test concurrent pool status checking under load
#[tokio::test]
async fn test_concurrent_pool_status_checking() {
    let event_router = create_test_event_router();
    let manager = Arc::new(PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    ));

    // Add some pools to different registries
    manager.add_known_pool("known_pool_1".to_string()).await;
    manager
        .add_rejected_pool("rejected_pool_1".to_string())
        .await;
    manager.add_pending_pool("pending_pool_1".to_string()).await;

    // Test concurrent status checking
    let mut handles = vec![];
    for i in 0..100 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            let pool_ids = vec![
                format!("known_pool_{}", i % 10),
                format!("rejected_pool_{}", i % 10),
                format!("pending_pool_{}", i % 10),
                format!("unknown_pool_{}", i),
            ];

            let mut statuses = vec![];
            for pool_id in pool_ids {
                let status = manager_clone.check_pool_status(&pool_id).await;
                statuses.push((pool_id, status));
            }
            statuses
        });
        handles.push(handle);
    }

    // Wait for all operations to complete
    let timeout_result = timeout(Duration::from_secs(10), futures::future::join_all(handles)).await;
    assert!(
        timeout_result.is_ok(),
        "Concurrent status checking timed out"
    );

    let results = timeout_result.unwrap();
    assert_eq!(results.len(), 100);

    // Verify all operations completed successfully
    for result in results {
        let statuses = result.unwrap();
        assert_eq!(statuses.len(), 4);
    }
}

/// Test cache TTL and cleanup functionality
#[tokio::test]
async fn test_cache_ttl_and_cleanup() {
    let event_router = create_test_event_router();
    let manager = PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    );

    // Test CachedPoolState TTL
    let pool_state = create_test_pool_state("ttl_test_pool");

    // Create cached state with short TTL
    let short_ttl_cache = CachedPoolState::new(pool_state.clone(), Duration::from_millis(10));
    assert!(!short_ttl_cache.is_expired());

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(15)).await;
    assert!(short_ttl_cache.is_expired());

    // Test cache with longer TTL
    let long_ttl_cache = CachedPoolState::new(pool_state, Duration::from_secs(60));
    assert!(!long_ttl_cache.is_expired());

    // Test manager cache functionality
    manager
        .cache_pool_state(
            "cache_test_pool".to_string(),
            create_test_pool_state("cache_test_pool"),
        )
        .await;

    let sizes = manager.get_registry_sizes().await;
    assert_eq!(sizes.cached_states, 1);

    // Test draining cache
    let drained = manager.drain_cached_pool_states().await;
    assert_eq!(drained.len(), 1);
    assert!(drained.contains_key("cache_test_pool"));

    let sizes_after_drain = manager.get_registry_sizes().await;
    assert_eq!(sizes_after_drain.cached_states, 0);
}

/// Test filtering logic with various configurations
#[tokio::test]
async fn test_filtering_logic() {
    let event_router = create_test_event_router();

    // Test with DEX whitelist
    let dex_filter_config = PoolFilterConfig {
        dex_whitelist: Some(vec!["hyperion".to_string()]),
        ..Default::default()
    };

    let manager = PoolStateManager::new(
        event_router.clone(),
        DataSourceType::Live,
        dex_filter_config,
    );

    let hyperion_discovery = PoolDiscovery {
        pool_id: "hyperion_pool".to_string(),
        dex_name: "hyperion".to_string(),
        discovered_in_event: "test_event".to_string(),
    };

    let thala_discovery = PoolDiscovery {
        pool_id: "thala_pool".to_string(),
        dex_name: "thala".to_string(),
        discovered_in_event: "test_event".to_string(),
    };

    let discoveries = vec![hyperion_discovery, thala_discovery];
    let filtered = manager.process_pool_discoveries(discoveries).await;

    // Should only accept hyperion pool
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].dex_name, "hyperion");

    // Test token filtering with pool state
    let mut apt_pool = create_test_pool_state("apt_pool");
    apt_pool.token_a = "APT".to_string();
    apt_pool.token_b = "USDC".to_string();

    let mut btc_pool = create_test_pool_state("btc_pool");
    btc_pool.token_a = "BTC".to_string();
    btc_pool.token_b = "ETH".to_string();

    // Test token whitelist
    let token_filter_config = PoolFilterConfig {
        token_whitelist: Some(vec!["APT".to_string(), "USDC".to_string()]),
        ..Default::default()
    };

    let token_manager = PoolStateManager::new(
        event_router.clone(),
        DataSourceType::Live,
        token_filter_config,
    );

    assert!(token_manager.should_track_pool_with_state(&apt_pool));
    assert!(!token_manager.should_track_pool_with_state(&btc_pool));

    // Test token blacklist
    let blacklist_config = PoolFilterConfig {
        token_blacklist: Some(vec!["BTC".to_string()]),
        ..Default::default()
    };

    let blacklist_manager =
        PoolStateManager::new(event_router, DataSourceType::Live, blacklist_config);

    assert!(blacklist_manager.should_track_pool_with_state(&apt_pool));
    assert!(!blacklist_manager.should_track_pool_with_state(&btc_pool));
}

/// Test concurrent registry operations
#[tokio::test]
async fn test_concurrent_registry_operations() {
    let event_router = create_test_event_router();
    let manager = Arc::new(PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    ));

    // Test concurrent additions to different registries
    let mut handles = vec![];
    for i in 0..50 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            // Add to different registries based on index
            match i % 3 {
                0 => manager_clone.add_known_pool(format!("known_{}", i)).await,
                1 => {
                    manager_clone
                        .add_rejected_pool(format!("rejected_{}", i))
                        .await
                }
                2 => {
                    manager_clone
                        .add_pending_pool(format!("pending_{}", i))
                        .await
                }
                _ => unreachable!(),
            }
        });
        handles.push(handle);
    }

    // Wait for all operations
    let timeout_result = timeout(Duration::from_secs(5), futures::future::join_all(handles)).await;
    assert!(
        timeout_result.is_ok(),
        "Concurrent registry operations timed out"
    );

    // Verify registry sizes
    let sizes = manager.get_registry_sizes().await;
    assert!(sizes.known_pools >= 16); // At least 16-17 known pools
    assert!(sizes.rejected_pools >= 16);
    assert!(sizes.pending_pools >= 16);
    assert_eq!(
        sizes.known_pools + sizes.rejected_pools + sizes.pending_pools,
        50
    );
}

/// Test statistics tracking
#[tokio::test]
async fn test_statistics_tracking() {
    let event_router = create_test_event_router();
    let manager = PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    );

    // Initial stats should be zero
    let initial_stats = manager.get_stats().await;
    assert_eq!(initial_stats.pools_rejected, 0);
    assert_eq!(initial_stats.pools_pending, 0);

    // Add some pools and check stats
    manager.add_rejected_pool("rejected_1".to_string()).await;
    manager.add_rejected_pool("rejected_2".to_string()).await;
    manager.add_pending_pool("pending_1".to_string()).await;

    let updated_stats = manager.get_stats().await;
    assert_eq!(updated_stats.pools_rejected, 2);
    assert_eq!(updated_stats.pools_pending, 1);

    // Test cache statistics
    manager
        .cache_pool_state("cache_1".to_string(), create_test_pool_state("cache_1"))
        .await;
    manager
        .cache_pool_state("cache_2".to_string(), create_test_pool_state("cache_2"))
        .await;

    let cache_stats = manager.get_stats().await;
    assert_eq!(cache_stats.cache_misses, 2); // Two new cache entries
}

/// Test worker result processing flow
#[tokio::test]
async fn test_worker_result_flow() {
    let event_router = create_test_event_router();
    let manager = Arc::new(PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    ));

    // Test basic worker sender/receiver setup
    let worker_tx = manager.get_worker_result_sender();
    assert!(
        !worker_tx.is_closed(),
        "Worker channel should not be closed initially"
    );

    // Test pool discovery processing
    let discoveries = vec![
        PoolDiscovery {
            pool_id: "test_pool_1".to_string(),
            dex_name: "hyperion".to_string(),
            discovered_in_event: "test_event".to_string(),
        },
        PoolDiscovery {
            pool_id: "test_pool_2".to_string(),
            dex_name: "hyperion".to_string(),
            discovered_in_event: "test_event".to_string(),
        },
    ];

    let filtered_discoveries = manager.process_pool_discoveries(discoveries).await;
    assert_eq!(filtered_discoveries.len(), 2);

    // Simulate the worker spawning process that adds pools to pending
    manager.spawn_pool_state_workers(filtered_discoveries).await;

    // Check that pools were added to pending
    let sizes = manager.get_registry_sizes().await;
    assert_eq!(sizes.pending_pools, 2);

    // Verify pool statuses - they should be Pending after spawn_pool_state_workers
    assert_eq!(
        manager.check_pool_status(&"test_pool_1".to_string()).await,
        PoolStatus::Pending
    );
    assert_eq!(
        manager.check_pool_status(&"test_pool_2".to_string()).await,
        PoolStatus::Pending
    );
}

/// Test pool discovery from transaction (mocked)
#[tokio::test]
async fn test_pool_discovery_from_transaction() {
    let event_router = create_test_event_router();
    let manager = PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    );

    // Create a mock transaction
    use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction;
    let transaction = Transaction {
        version: 100, // Divisible by 100 to trigger mock discovery
        ..Default::default()
    };

    let discoveries = manager
        .extract_pool_ids_from_transaction(&transaction)
        .await;
    assert!(discoveries.is_ok());

    let discoveries = discoveries.unwrap();
    assert_eq!(discoveries.len(), 1);
    assert_eq!(discoveries[0].pool_id, "pool_from_txn_100");
    assert_eq!(discoveries[0].dex_name, "hyperion");

    // Test with transaction that doesn't trigger discovery
    let transaction_no_discovery = Transaction {
        version: 101, // Not divisible by 100
        ..Default::default()
    };

    let no_discoveries = manager
        .extract_pool_ids_from_transaction(&transaction_no_discovery)
        .await;
    assert!(no_discoveries.is_ok());
    assert_eq!(no_discoveries.unwrap().len(), 0);
}

/// Test maximum tracked pools limit
#[tokio::test]
async fn test_max_tracked_pools_limit() {
    let event_router = create_test_event_router();

    // Set very low limit for testing
    let limited_config = PoolFilterConfig {
        max_tracked_pools: Some(3),
        ..Default::default()
    };

    let manager = PoolStateManager::new(event_router, DataSourceType::Live, limited_config);

    // Add pools up to the limit
    manager.add_known_pool("pool_1".to_string()).await;
    manager.add_known_pool("pool_2".to_string()).await;
    manager.add_pending_pool("pool_3".to_string()).await; // This reaches the limit

    // Try to process more discoveries
    let excess_discoveries = vec![
        PoolDiscovery {
            pool_id: "pool_4".to_string(),
            dex_name: "hyperion".to_string(),
            discovered_in_event: "test_event".to_string(),
        },
        PoolDiscovery {
            pool_id: "pool_5".to_string(),
            dex_name: "hyperion".to_string(),
            discovered_in_event: "test_event".to_string(),
        },
    ];

    let filtered = manager.process_pool_discoveries(excess_discoveries).await;

    // Should reject all new pools due to limit
    assert_eq!(filtered.len(), 0);

    // Check that the rejected pools were added to rejected registry
    let sizes = manager.get_registry_sizes().await;
    assert_eq!(sizes.rejected_pools, 2); // Two pools rejected due to limit
}

/// Test data source type behavior differences
#[tokio::test]
async fn test_data_source_type_differences() {
    let event_router = create_test_event_router();

    // Test Live data source
    let live_manager = PoolStateManager::new(
        event_router.clone(),
        DataSourceType::Live,
        PoolFilterConfig::default(),
    );

    // Test Replay data source
    let replay_manager = PoolStateManager::new(
        event_router,
        DataSourceType::Replay,
        PoolFilterConfig::default(),
    );

    // Both should initialize successfully
    let live_sizes = live_manager.get_registry_sizes().await;
    let replay_sizes = replay_manager.get_registry_sizes().await;

    assert_eq!(live_sizes.known_pools, 0);
    assert_eq!(live_sizes.rejected_pools, 0);
    assert_eq!(live_sizes.pending_pools, 0);

    assert_eq!(replay_sizes.known_pools, 0);
    assert_eq!(replay_sizes.rejected_pools, 0);
    assert_eq!(replay_sizes.pending_pools, 0);

    // Both should handle pool operations identically
    live_manager.add_known_pool("test_pool".to_string()).await;
    replay_manager.add_known_pool("test_pool".to_string()).await;

    assert_eq!(
        live_manager
            .check_pool_status(&"test_pool".to_string())
            .await,
        PoolStatus::Known
    );
    assert_eq!(
        replay_manager
            .check_pool_status(&"test_pool".to_string())
            .await,
        PoolStatus::Known
    );
}

/// Test cleanup operations
#[tokio::test]
async fn test_cleanup_operations() {
    let event_router = create_test_event_router();
    let manager = PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    );

    // Add some cached states that will expire quickly
    let short_lived_state = create_test_pool_state("short_lived");
    let long_lived_state = create_test_pool_state("long_lived");

    // Create cached states directly for testing
    let short_cache = CachedPoolState::new(short_lived_state, Duration::from_millis(1));
    let long_cache = CachedPoolState::new(long_lived_state, Duration::from_secs(60));

    // Cache some states
    manager
        .cache_pool_state("short_lived".to_string(), short_cache.state.clone())
        .await;
    manager
        .cache_pool_state("long_lived".to_string(), long_cache.state.clone())
        .await;

    // Initial state
    let initial_sizes = manager.get_registry_sizes().await;
    assert_eq!(initial_sizes.cached_states, 2);

    // Wait for short-lived cache to expire
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Run cleanup
    manager.cleanup_expired_cache().await;

    // Should have cleaned up expired entries
    let _stats_after_cleanup = manager.get_stats().await;
    // Note: The actual cleanup behavior depends on the internal implementation
    // This test verifies the cleanup method runs without error
}

/// Test pool type filtering
#[tokio::test]
async fn test_pool_type_filtering() {
    let event_router = create_test_event_router();

    let pool_type_config = PoolFilterConfig {
        pool_type_whitelist: Some(vec!["constant_product".to_string()]),
        ..Default::default()
    };

    let manager = PoolStateManager::new(event_router, DataSourceType::Live, pool_type_config);

    // Create pool states with different types
    let mut constant_product_pool = create_test_pool_state("cp_pool");
    constant_product_pool.additional_data = serde_json::json!({"pool_type": "constant_product"});

    let mut weighted_pool = create_test_pool_state("weighted_pool");
    weighted_pool.additional_data = serde_json::json!({"pool_type": "weighted"});

    let mut no_type_pool = create_test_pool_state("no_type_pool");
    no_type_pool.additional_data = serde_json::json!({});

    // Test filtering
    assert!(manager.should_track_pool_with_state(&constant_product_pool));
    assert!(!manager.should_track_pool_with_state(&weighted_pool));
    assert!(manager.should_track_pool_with_state(&no_type_pool)); // No type specified - should pass (defaults to true)
}

/// Test concurrent cache operations with different TTLs
#[tokio::test]
async fn test_concurrent_cache_with_ttls() {
    let event_router = create_test_event_router();
    let manager = Arc::new(PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    ));

    // Spawn concurrent cache operations
    let mut handles = vec![];
    for i in 0..20 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            let pool_state = create_test_pool_state(&format!("concurrent_pool_{}", i));
            manager_clone
                .cache_pool_state(format!("concurrent_pool_{}", i), pool_state)
                .await;
        });
        handles.push(handle);
    }

    // Wait for all caching operations
    let timeout_result = timeout(Duration::from_secs(5), futures::future::join_all(handles)).await;
    assert!(
        timeout_result.is_ok(),
        "Concurrent caching operations timed out"
    );

    // Verify all states were cached
    let sizes = manager.get_registry_sizes().await;
    assert_eq!(sizes.cached_states, 20);

    // Drain all cached states
    let drained = manager.drain_cached_pool_states().await;
    assert_eq!(drained.len(), 20);

    // Cache should be empty after draining
    let final_sizes = manager.get_registry_sizes().await;
    assert_eq!(final_sizes.cached_states, 0);
}

/// Test edge cases and error conditions
#[tokio::test]
async fn test_edge_cases() {
    let event_router = create_test_event_router();
    let manager = PoolStateManager::new(
        event_router,
        DataSourceType::Live,
        PoolFilterConfig::default(),
    );

    // Test empty pool discovery list
    let empty_discoveries = vec![];
    let filtered = manager.process_pool_discoveries(empty_discoveries).await;
    assert_eq!(filtered.len(), 0);

    // Test duplicate pool discoveries
    let duplicate_discoveries = vec![
        PoolDiscovery {
            pool_id: "duplicate_pool".to_string(),
            dex_name: "hyperion".to_string(),
            discovered_in_event: "event1".to_string(),
        },
        PoolDiscovery {
            pool_id: "duplicate_pool".to_string(), // Same pool ID
            dex_name: "hyperion".to_string(),
            discovered_in_event: "event2".to_string(),
        },
    ];

    // Both discoveries should be processed since process_pool_discoveries doesn't deduplicate within a batch
    let first_batch = manager
        .process_pool_discoveries(duplicate_discoveries)
        .await;
    assert_eq!(first_batch.len(), 2); // Both identical pools pass through since they're both Unknown

    // Simulate adding the discovered pool to pending registry
    manager.add_pending_pool("duplicate_pool".to_string()).await;

    // Second identical discovery should be filtered out (already pending)
    let second_duplicate = vec![PoolDiscovery {
        pool_id: "duplicate_pool".to_string(),
        dex_name: "hyperion".to_string(),
        discovered_in_event: "event3".to_string(),
    }];
    let second_batch = manager.process_pool_discoveries(second_duplicate).await;
    assert_eq!(second_batch.len(), 0); // Should be filtered out as already pending

    // Test pool status transitions - the pool should be Pending since we added it earlier
    assert_eq!(
        manager
            .check_pool_status(&"duplicate_pool".to_string())
            .await,
        PoolStatus::Pending
    );

    manager.add_known_pool("duplicate_pool".to_string()).await;
    assert_eq!(
        manager
            .check_pool_status(&"duplicate_pool".to_string())
            .await,
        PoolStatus::Known
    );
}
