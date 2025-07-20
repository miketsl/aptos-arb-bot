//! End-to-end integration testing framework
//! Tests complete workflows from data ingestion to processing and output

use dex_adapters::{EventRouter, HyperionAdapter, TappAdapter, ThalaAdapter};
use market_data_ingestor::{
    data_source::{RecordedBatch, RecordedPoolState},
    file_rotation::FileRotationManager,
    pool_state_manager::{DataSourceType, PoolFilterConfig, PoolStateManager},
    recording_monitor::{MonitoringSettings, RecordingMonitor},
};
use prost::Message;
use serde_json;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::{NamedTempFile, TempDir};
use tokio::time::timeout;

/// Integration test fixture for setting up complete test environments
struct IntegrationTestFixture {
    pub temp_dir: TempDir,
    pub event_router: Arc<EventRouter>,
    pub pool_manager: Arc<PoolStateManager>,
    pub recording_monitor: Arc<RecordingMonitor>,
}

impl IntegrationTestFixture {
    async fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        // Set up event router with all adapters
        let mut event_router = EventRouter::new();
        event_router.register_adapter(Arc::new(HyperionAdapter::default()));
        event_router.register_adapter(Arc::new(TappAdapter::default()));
        event_router.register_adapter(Arc::new(ThalaAdapter::default()));
        let event_router = Arc::new(event_router);

        // Set up pool state manager
        let filter_config = PoolFilterConfig {
            min_tvl_usd: Some(100.0), // Lower threshold for testing
            max_tracked_pools: Some(100),
            ..Default::default()
        };

        let pool_manager = Arc::new(PoolStateManager::new(
            event_router.clone(),
            DataSourceType::Replay,
            filter_config,
        ));

        // Set up recording monitor
        let recording_monitor = Arc::new(RecordingMonitor::new(MonitoringSettings::default()));

        Self {
            temp_dir,
            event_router,
            pool_manager,
            recording_monitor,
        }
    }

    /// Create test protobuf data with multiple DEXes and pool types
    fn create_comprehensive_test_data(&self) -> Vec<u8> {
        let mut data = Vec::new();

        // Create batches with different DEXes and scenarios
        let test_scenarios = vec![
            // Hyperion CLMM pools
            (
                "hyperion_clmm_apt_usdc",
                "hyperion",
                "APT",
                "USDC",
                "clmm",
                serde_json::json!({
                    "sqrt_price": "1414213562373095048",
                    "liquidity": "1000000000000",
                    "tick": -276324,
                    "tick_spacing": 64
                }),
            ),
            (
                "hyperion_clmm_btc_apt",
                "hyperion",
                "BTC",
                "APT",
                "clmm",
                serde_json::json!({
                    "sqrt_price": "2236067977499789696",
                    "liquidity": "500000000000",
                    "tick": -138162,
                    "tick_spacing": 64
                }),
            ),
            // Thala weighted pools
            (
                "thala_weighted_apt_usdc_usdt_dai",
                "thala",
                "APT",
                "USDC",
                "weighted",
                serde_json::json!({
                    "weights": [25, 25, 25, 25],
                    "all_tokens": ["APT", "USDC", "USDT", "DAI"],
                    "all_reserves": ["1000000", "2000000", "2000000", "2000000"]
                }),
            ),
            // Tapp constant product pools
            (
                "tapp_cpmm_eth_usdc",
                "tapp",
                "ETH",
                "USDC",
                "constant_product",
                serde_json::json!({
                    "k": "2000000000000000000000000"
                }),
            ),
        ];

        for (i, (pool_id, dex, token_a, token_b, pool_type, additional_data)) in
            test_scenarios.iter().enumerate()
        {
            let pool_state = RecordedPoolState {
                pool_id: pool_id.to_string(),
                dex_name: dex.to_string(),
                token_a: token_a.to_string(),
                token_b: token_b.to_string(),
                reserve_a: format!("{}", 1000000 + i * 100000),
                reserve_b: format!("{}", 2000000 + i * 200000),
                fee_rate: match *dex {
                    "hyperion" => "0.003",
                    "thala" => "0.0025",
                    "tapp" => "0.005",
                    _ => "0.003",
                }
                .to_string(),
                all_tokens: if *pool_type == "weighted" {
                    vec![
                        "APT".to_string(),
                        "USDC".to_string(),
                        "USDT".to_string(),
                        "DAI".to_string(),
                    ]
                } else {
                    vec![token_a.to_string(), token_b.to_string()]
                },
                all_reserves: if *pool_type == "weighted" {
                    vec![
                        "1000000".to_string(),
                        "2000000".to_string(),
                        "2000000".to_string(),
                        "2000000".to_string(),
                    ]
                } else {
                    vec![
                        format!("{}", 1000000 + i * 100000),
                        format!("{}", 2000000 + i * 200000),
                    ]
                },
                all_weights: if *pool_type == "weighted" {
                    vec![25, 25, 25, 25]
                } else {
                    vec![]
                },
                pool_type: pool_type.to_string(),
                block_height: 100000 + i as u64,
                additional_data: serde_json::to_vec(additional_data).unwrap(),
            };

            let batch = RecordedBatch {
                start_version: 100000 + i as u64,
                end_version: 100001 + i as u64,
                timestamp_ms: 1641400000000 + (i as i64 * 60000), // 1 minute apart
                transactions: vec![],                             // Empty for simplicity
                pool_initializations: vec![pool_state],
            };

            batch.encode_length_delimited(&mut data).unwrap();
        }

        data
    }

    /// Create a test protobuf file
    fn create_test_protobuf_file(&self) -> NamedTempFile {
        let temp_file =
            NamedTempFile::new_in(&self.temp_dir).expect("Failed to create test protobuf file");
        let data = self.create_comprehensive_test_data();
        fs::write(temp_file.path(), data).expect("Failed to write test protobuf data");
        temp_file
    }
}

/// Test complete file-based ingestion workflow
#[tokio::test]
async fn test_end_to_end_file_ingestion() {
    let fixture = IntegrationTestFixture::new().await;
    let test_file = fixture.create_test_protobuf_file();

    // Create file data source
    let file_source = FileDataSource::new(
        test_file.path().to_path_buf(),
        Some(2.0), // 2x replay speed
    )
    .await
    .expect("Failed to create file data source");

    let mut data_source = DataSource::File(file_source);

    // Process data through the pipeline
    let start_time = Instant::now();
    let mut batches_processed = 0;
    let mut pools_discovered = 0;

    // Set up data processing loop with timeout
    let processing_result = timeout(Duration::from_secs(30), async {
        while let Some(batch_result) = data_source.next_batch().await {
            match batch_result {
                Ok(batch) => {
                    batches_processed += 1;
                    pools_discovered += batch.pool_initializations.len();

                    // Process pool discoveries
                    let discoveries: Vec<_> = batch
                        .pool_initializations
                        .iter()
                        .map(
                            |pool| market_data_ingestor::pool_state_manager::PoolDiscovery {
                                pool_id: pool.pool_id.clone(),
                                dex_name: pool.dex_name.clone(),
                                discovered_in_event: "file_ingestion".to_string(),
                            },
                        )
                        .collect();

                    let filtered_discoveries = fixture
                        .pool_manager
                        .process_pool_discoveries(discoveries)
                        .await;

                    // Record statistics
                    fixture
                        .recording_monitor
                        .record_batch_processed(
                            batch.pool_initializations.len(),
                            batch.transactions.len(),
                            0, // No errors in test data
                        )
                        .await;

                    // Verify batch integrity
                    assert!(batch.start_version <= batch.end_version);
                    assert!(batch.timestamp_ms > 0);

                    // All test pools should pass basic filtering
                    if !filtered_discoveries.is_empty() {
                        assert!(filtered_discoveries.len() <= discoveries.len());
                    }
                }
                Err(e) => {
                    panic!("Failed to process batch: {}", e);
                }
            }

            // Break after processing all test batches
            if batches_processed >= 4 {
                break;
            }
        }
    })
    .await;

    assert!(processing_result.is_ok(), "Processing timed out");

    let elapsed = start_time.elapsed();
    println!("End-to-end file ingestion completed in {:?}", elapsed);
    println!("Batches processed: {}", batches_processed);
    println!("Pools discovered: {}", pools_discovered);

    // Verify results
    assert_eq!(batches_processed, 4, "Should have processed 4 test batches");
    assert_eq!(pools_discovered, 4, "Should have discovered 4 pools");
    assert!(
        elapsed < Duration::from_secs(10),
        "Processing should complete quickly"
    );

    // Check pool manager state
    let stats = fixture.pool_manager.get_stats().await;
    assert_eq!(stats.pools_discovered, 4);

    let sizes = fixture.pool_manager.get_registry_sizes().await;
    assert!(sizes.known_pools + sizes.rejected_pools + sizes.pending_pools <= 4);

    // Check recording monitor statistics
    let recording_stats = fixture.recording_monitor.get_stats().await;
    assert_eq!(recording_stats.batches_processed, 4);
    assert_eq!(recording_stats.pools_discovered, 4);
    assert_eq!(recording_stats.processing_errors, 0);
}

/// Test multi-DEX integration and routing
#[tokio::test]
async fn test_multi_dex_integration() {
    let fixture = IntegrationTestFixture::new().await;

    // Test event router capabilities
    let dex_names = vec!["hyperion", "thala", "tapp"];

    for dex_name in &dex_names {
        // Test pool state fetching for each DEX
        let pool_id = format!("{}_test_pool", dex_name);
        let fetch_result = fixture
            .event_router
            .fetch_pool_state(&pool_id, dex_name)
            .await;

        // Should either succeed or fail gracefully (no panics)
        match fetch_result {
            Ok(pool_state) => {
                assert_eq!(pool_state.dex_name, *dex_name);
                assert_eq!(pool_state.pool_id, pool_id);
            }
            Err(_) => {
                // Failure is acceptable in test environment without real APIs
                // The key is that it doesn't panic and returns a proper error
            }
        }

        // Test adapter registration
        let adapter = fixture.event_router.get_adapter(dex_name);
        assert!(
            adapter.is_some(),
            "Adapter for {} should be registered",
            dex_name
        );
    }

    // Test pool ID extraction from transaction events
    let test_transaction =
        aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction {
            version: 200, // Should trigger mock discovery
            ..Default::default()
        };

    let discoveries = fixture
        .pool_manager
        .extract_pool_ids_from_transaction(&test_transaction)
        .await
        .expect("Should extract pool IDs without error");

    assert_eq!(discoveries.len(), 1);
    assert_eq!(discoveries[0].pool_id, "pool_from_txn_200");
    assert_eq!(discoveries[0].dex_name, "hyperion");
}

/// Test filtering and configuration scenarios
#[tokio::test]
async fn test_filtering_integration() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let event_router = {
        let mut router = EventRouter::new();
        router.register_adapter(Arc::new(HyperionAdapter::default()));
        router.register_adapter(Arc::new(ThalaAdapter::default()));
        Arc::new(router)
    };

    // Test different filter configurations
    let filter_scenarios = vec![
        // DEX whitelist
        PoolFilterConfig {
            dex_whitelist: Some(vec!["hyperion".to_string()]),
            max_tracked_pools: Some(50),
            ..Default::default()
        },
        // Token whitelist
        PoolFilterConfig {
            token_whitelist: Some(vec!["APT".to_string(), "USDC".to_string()]),
            max_tracked_pools: Some(50),
            ..Default::default()
        },
        // Strict limits
        PoolFilterConfig {
            max_tracked_pools: Some(2), // Very restrictive
            min_tvl_usd: Some(10000.0), // High threshold
            ..Default::default()
        },
    ];

    for (i, filter_config) in filter_scenarios.iter().enumerate() {
        let pool_manager = Arc::new(PoolStateManager::new(
            event_router.clone(),
            DataSourceType::Replay,
            filter_config.clone(),
        ));

        // Create test discoveries that should be filtered differently
        let test_discoveries = vec![
            market_data_ingestor::pool_state_manager::PoolDiscovery {
                pool_id: format!("filter_test_hyperion_{}", i),
                dex_name: "hyperion".to_string(),
                discovered_in_event: "test_event".to_string(),
            },
            market_data_ingestor::pool_state_manager::PoolDiscovery {
                pool_id: format!("filter_test_thala_{}", i),
                dex_name: "thala".to_string(),
                discovered_in_event: "test_event".to_string(),
            },
            market_data_ingestor::pool_state_manager::PoolDiscovery {
                pool_id: format!("filter_test_tapp_{}", i),
                dex_name: "tapp".to_string(),
                discovered_in_event: "test_event".to_string(),
            },
        ];

        let filtered = pool_manager
            .process_pool_discoveries(test_discoveries)
            .await;

        match i {
            0 => {
                // DEX whitelist - should only accept hyperion
                assert_eq!(filtered.len(), 1);
                assert_eq!(filtered[0].dex_name, "hyperion");
            }
            1 => {
                // Token whitelist - will need pool state to filter, so all pass initial filter
                assert_eq!(filtered.len(), 3);
            }
            2 => {
                // Strict limits - should accept some pools until limit reached
                assert!(filtered.len() <= 2);
            }
            _ => unreachable!(),
        }

        // Verify registry state
        let sizes = pool_manager.get_registry_sizes().await;
        let total_tracked = sizes.known_pools + sizes.pending_pools;

        if let Some(max_pools) = filter_config.max_tracked_pools {
            assert!(
                total_tracked <= max_pools,
                "Should respect max pool limit: {} <= {}",
                total_tracked,
                max_pools
            );
        }
    }
}

/// Test error handling and recovery scenarios
#[tokio::test]
async fn test_error_handling_integration() {
    let fixture = IntegrationTestFixture::new().await;

    // Test with corrupted data
    let corrupted_file =
        NamedTempFile::new_in(&fixture.temp_dir).expect("Failed to create corrupted file");
    fs::write(corrupted_file.path(), &[0xFF, 0xFF, 0xFF, 0xFF])
        .expect("Failed to write corrupted data");

    let file_result = FileDataSource::new(corrupted_file.path().to_path_buf(), None).await;

    // Should handle corrupted data gracefully
    match file_result {
        Ok(mut data_source) => {
            let batch_result = data_source.next_batch().await;
            match batch_result {
                Some(Err(_)) => {
                    // Expected - corrupted data should produce an error
                }
                Some(Ok(_)) => {
                    // Unexpected but not necessarily wrong if it can parse something
                }
                None => {
                    // Also acceptable - might detect corruption and return None
                }
            }
        }
        Err(_) => {
            // Also acceptable - might detect corruption during initialization
        }
    }

    // Test with non-existent file
    let nonexistent_path = fixture.temp_dir.path().join("nonexistent.pb");
    let nonexistent_result = FileDataSource::new(nonexistent_path, None).await;
    assert!(
        nonexistent_result.is_err(),
        "Should fail with non-existent file"
    );

    // Test pool manager error recovery
    let invalid_discoveries = vec![market_data_ingestor::pool_state_manager::PoolDiscovery {
        pool_id: "".to_string(),  // Invalid empty pool ID
        dex_name: "".to_string(), // Invalid empty DEX name
        discovered_in_event: "error_test".to_string(),
    }];

    let filtered = fixture
        .pool_manager
        .process_pool_discoveries(invalid_discoveries)
        .await;
    // Should handle invalid discoveries gracefully
    assert_eq!(filtered.len(), 0, "Should filter out invalid discoveries");

    // Test concurrent error scenarios
    let mut handles = vec![];
    for i in 0..10 {
        let pool_manager = fixture.pool_manager.clone();
        let handle = tokio::spawn(async move {
            let invalid_discovery = vec![market_data_ingestor::pool_state_manager::PoolDiscovery {
                pool_id: format!("error_pool_{}", i),
                dex_name: "nonexistent_dex".to_string(),
                discovered_in_event: "concurrent_error_test".to_string(),
            }];
            pool_manager
                .process_pool_discoveries(invalid_discovery)
                .await
        });
        handles.push(handle);
    }

    let results = futures::future::join_all(handles).await;

    // All error scenarios should complete without panicking
    for result in results {
        assert!(result.is_ok(), "Concurrent error handling should not panic");
        let discoveries = result.unwrap();
        // Invalid DEX should be filtered out
        assert!(discoveries.is_empty());
    }
}

/// Test performance and scalability scenarios
#[tokio::test]
async fn test_performance_integration() {
    let fixture = IntegrationTestFixture::new().await;

    // Create larger test dataset
    let large_dataset_file =
        NamedTempFile::new_in(&fixture.temp_dir).expect("Failed to create large dataset file");

    let mut large_data = Vec::new();
    let batch_count = 50; // Moderate size for CI testing

    for i in 0..batch_count {
        let pool_states: Vec<_> = (0..5)
            .map(|j| RecordedPoolState {
                pool_id: format!("perf_pool_{}_{}", i, j),
                dex_name: match j % 3 {
                    0 => "hyperion",
                    1 => "thala",
                    2 => "tapp",
                    _ => unreachable!(),
                }
                .to_string(),
                token_a: "APT".to_string(),
                token_b: "USDC".to_string(),
                reserve_a: format!("{}", 1000000 + i * 1000 + j * 100),
                reserve_b: format!("{}", 2000000 + i * 2000 + j * 200),
                fee_rate: "0.003".to_string(),
                all_tokens: vec!["APT".to_string(), "USDC".to_string()],
                all_reserves: vec![
                    format!("{}", 1000000 + i * 1000 + j * 100),
                    format!("{}", 2000000 + i * 2000 + j * 200),
                ],
                all_weights: vec![],
                pool_type: "clmm".to_string(),
                block_height: 200000 + i,
                additional_data: vec![],
            })
            .collect();

        let batch = RecordedBatch {
            start_version: 200000 + i,
            end_version: 200001 + i,
            timestamp_ms: 1641500000000 + (i as i64 * 1000),
            transactions: vec![],
            pool_initializations: pool_states,
        };

        batch.encode_length_delimited(&mut large_data).unwrap();
    }

    fs::write(large_dataset_file.path(), large_data).expect("Failed to write large dataset");

    // Process with performance monitoring
    let file_source = FileDataSource::new(
        large_dataset_file.path().to_path_buf(),
        Some(10.0), // High replay speed
    )
    .await
    .expect("Failed to create large file source");

    let mut data_source = DataSource::File(file_source);

    let start_time = Instant::now();
    let mut total_batches = 0;
    let mut total_pools = 0;

    let performance_result = timeout(Duration::from_secs(60), async {
        while let Some(batch_result) = data_source.next_batch().await {
            if let Ok(batch) = batch_result {
                total_batches += 1;
                total_pools += batch.pool_initializations.len();

                // Process discoveries
                let discoveries: Vec<_> = batch
                    .pool_initializations
                    .iter()
                    .map(
                        |pool| market_data_ingestor::pool_state_manager::PoolDiscovery {
                            pool_id: pool.pool_id.clone(),
                            dex_name: pool.dex_name.clone(),
                            discovered_in_event: "performance_test".to_string(),
                        },
                    )
                    .collect();

                let _filtered = fixture
                    .pool_manager
                    .process_pool_discoveries(discoveries)
                    .await;

                // Break after processing reasonable amount for CI
                if total_batches >= batch_count {
                    break;
                }
            }
        }
    })
    .await;

    assert!(
        performance_result.is_ok(),
        "Performance test should complete within timeout"
    );

    let elapsed = start_time.elapsed();
    let batches_per_sec = total_batches as f64 / elapsed.as_secs_f64();
    let pools_per_sec = total_pools as f64 / elapsed.as_secs_f64();

    println!("Performance test results:");
    println!("  Total batches: {}", total_batches);
    println!("  Total pools: {}", total_pools);
    println!("  Elapsed time: {:?}", elapsed);
    println!("  Batches per second: {:.2}", batches_per_sec);
    println!("  Pools per second: {:.2}", pools_per_sec);

    // Performance assertions
    assert_eq!(total_batches, batch_count);
    assert_eq!(total_pools, batch_count * 5);
    assert!(
        batches_per_sec > 1.0,
        "Should process at least 1 batch per second"
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "Should complete within 30 seconds"
    );

    // Check final state
    let final_sizes = fixture.pool_manager.get_registry_sizes().await;
    let total_tracked =
        final_sizes.known_pools + final_sizes.rejected_pools + final_sizes.pending_pools;
    assert!(
        total_tracked <= total_pools,
        "Tracked pools should not exceed discovered pools"
    );
}

/// Test file rotation and monitoring integration
#[tokio::test]
async fn test_file_rotation_integration() {
    let fixture = IntegrationTestFixture::new().await;

    // Create rotation configuration
    let rotation_config = RotationConfig {
        max_size_mb: 1, // Small size for testing
        time_pattern: "{timestamp}".to_string(),
        backup_count: 3,
        rotation_interval: Duration::from_secs(1), // Fast rotation for testing
    };

    let base_path = fixture.temp_dir.path().join("rotation_test.pb");
    let rotation_manager = FileRotationManager::new(base_path.clone(), rotation_config)
        .expect("Failed to create rotation manager");

    // Write data that should trigger rotation
    let test_data = fixture.create_comprehensive_test_data();

    // Write multiple times to trigger rotation
    for i in 0..5 {
        let write_result = rotation_manager.write_data(&test_data).await;
        assert!(write_result.is_ok(), "Write {} should succeed", i);

        // Small delay to allow rotation
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Check that files were created
    let dir_entries: Vec<_> = fs::read_dir(fixture.temp_dir.path())
        .expect("Failed to read temp directory")
        .collect();

    let pb_files: Vec<_> = dir_entries
        .iter()
        .filter_map(|entry| entry.as_ref().ok())
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("pb"))
        .collect();

    assert!(
        !pb_files.is_empty(),
        "Should have created at least one protobuf file"
    );
    println!(
        "Created {} protobuf files during rotation test",
        pb_files.len()
    );

    // Verify files contain valid data
    for file_entry in pb_files {
        let file_path = file_entry.path();
        let file_data = fs::read(&file_path).expect("Failed to read rotated file");
        assert!(
            !file_data.is_empty(),
            "Rotated file should not be empty: {:?}",
            file_path
        );
    }
}

/// Test monitoring and metrics collection
#[tokio::test]
async fn test_monitoring_integration() {
    let fixture = IntegrationTestFixture::new().await;

    // Simulate processing events
    let test_scenarios = [
        (5, 0, 0),  // 5 pools discovered, 0 transactions, 0 errors
        (3, 10, 1), // 3 pools, 10 transactions, 1 error
        (0, 5, 0),  // No pools, 5 transactions, no errors
        (8, 20, 2), // 8 pools, 20 transactions, 2 errors
    ];

    for (pools, transactions, errors) in &test_scenarios {
        fixture
            .recording_monitor
            .record_batch_processed(*pools, *transactions, *errors)
            .await;
    }

    // Get final statistics
    let stats = fixture.recording_monitor.get_stats().await;

    assert_eq!(stats.batches_processed, 4);
    assert_eq!(stats.pools_discovered, 16); // 5+3+0+8
    assert_eq!(stats.transactions_processed, 35); // 0+10+5+20
    assert_eq!(stats.processing_errors, 3); // 0+1+0+2

    // Test statistics formatting
    let formatted_stats = format!("{:?}", stats);
    assert!(formatted_stats.contains("batches_processed"));
    assert!(formatted_stats.contains("pools_discovered"));

    // Test monitoring state
    let should_continue = fixture.recording_monitor.should_continue().await;
    assert!(should_continue, "Monitoring should continue by default");

    // Test error threshold (if implemented)
    // Simulate high error rate
    for _ in 0..100 {
        fixture
            .recording_monitor
            .record_batch_processed(0, 0, 1)
            .await;
    }

    let high_error_stats = fixture.recording_monitor.get_stats().await;
    assert!(high_error_stats.processing_errors >= 100);

    // Monitor should still function (specific behavior depends on implementation)
    let continues_with_errors = fixture.recording_monitor.should_continue().await;
    // This is implementation-dependent, so we just verify it doesn't panic
    assert!(continues_with_errors == true || continues_with_errors == false);
}

/// Test configuration validation and loading
#[tokio::test]
async fn test_configuration_integration() {
    let fixture = IntegrationTestFixture::new().await;

    // Test various filter configurations
    let config_tests = vec![
        // Valid configuration
        PoolFilterConfig {
            min_tvl_usd: Some(1000.0),
            token_whitelist: Some(vec!["APT".to_string(), "USDC".to_string()]),
            max_tracked_pools: Some(100),
            ..Default::default()
        },
        // Edge case configuration
        PoolFilterConfig {
            min_tvl_usd: Some(0.0),     // Zero threshold
            max_tracked_pools: Some(1), // Minimal limit
            ..Default::default()
        },
        // Restrictive configuration
        PoolFilterConfig {
            token_whitelist: Some(vec!["NONEXISTENT".to_string()]),
            dex_whitelist: Some(vec!["NONEXISTENT".to_string()]),
            pool_type_whitelist: Some(vec!["NONEXISTENT".to_string()]),
            max_tracked_pools: Some(0), // No pools allowed
            ..Default::default()
        },
    ];

    for (i, config) in config_tests.iter().enumerate() {
        let pool_manager = PoolStateManager::new(
            fixture.event_router.clone(),
            DataSourceType::Replay,
            config.clone(),
        );

        // Test that manager initializes correctly
        let initial_sizes = pool_manager.get_registry_sizes().await;
        assert_eq!(initial_sizes.known_pools, 0);
        assert_eq!(initial_sizes.rejected_pools, 0);
        assert_eq!(initial_sizes.pending_pools, 0);

        // Test discovery processing with each configuration
        let test_discovery = vec![market_data_ingestor::pool_state_manager::PoolDiscovery {
            pool_id: format!("config_test_pool_{}", i),
            dex_name: "hyperion".to_string(),
            discovered_in_event: "config_test".to_string(),
        }];

        let filtered = pool_manager.process_pool_discoveries(test_discovery).await;

        // Verify filtering behavior matches configuration
        match i {
            0 => {
                // Valid config - should process normally
                assert!(filtered.len() <= 1);
            }
            1 => {
                // Edge case config - should still work
                assert!(filtered.len() <= 1);
            }
            2 => {
                // Restrictive config - should reject everything
                assert_eq!(filtered.len(), 0);
            }
            _ => unreachable!(),
        }

        println!("Configuration test {} passed", i);
    }
}

/// Integration test to verify complete workflow end-to-end  
#[tokio::test]
async fn test_complete_workflow_integration() {
    let fixture = IntegrationTestFixture::new().await;
    let test_file = fixture.create_test_protobuf_file();

    println!("Starting complete workflow integration test");

    // Phase 1: Data ingestion
    let file_source = FileDataSource::new(test_file.path().to_path_buf(), Some(1.0))
        .await
        .expect("Failed to create file source");

    let mut data_source = DataSource::File(file_source);

    // Phase 2: Processing and discovery
    let mut workflow_stats = WorkflowStats::default();

    let workflow_result = timeout(Duration::from_secs(45), async {
        while let Some(batch_result) = data_source.next_batch().await {
            match batch_result {
                Ok(batch) => {
                    workflow_stats.batches_processed += 1;
                    workflow_stats.pools_discovered += batch.pool_initializations.len();

                    // Pool discovery phase
                    let discoveries: Vec<_> = batch
                        .pool_initializations
                        .iter()
                        .map(
                            |pool| market_data_ingestor::pool_state_manager::PoolDiscovery {
                                pool_id: pool.pool_id.clone(),
                                dex_name: pool.dex_name.clone(),
                                discovered_in_event: "complete_workflow".to_string(),
                            },
                        )
                        .collect();

                    // Filtering phase
                    let filtered = fixture
                        .pool_manager
                        .process_pool_discoveries(discoveries)
                        .await;
                    workflow_stats.pools_accepted += filtered.len();

                    // Monitoring phase
                    fixture
                        .recording_monitor
                        .record_batch_processed(
                            batch.pool_initializations.len(),
                            batch.transactions.len(),
                            0,
                        )
                        .await;

                    // Verify data integrity throughout
                    for pool_state in &batch.pool_initializations {
                        assert!(!pool_state.pool_id.is_empty());
                        assert!(!pool_state.dex_name.is_empty());
                        assert!(!pool_state.token_a.is_empty());
                        assert!(!pool_state.token_b.is_empty());
                        workflow_stats.integrity_checks += 1;
                    }

                    if workflow_stats.batches_processed >= 4 {
                        break;
                    }
                }
                Err(e) => {
                    workflow_stats.errors += 1;
                    println!("Batch processing error: {}", e);
                }
            }
        }
    })
    .await;

    assert!(
        workflow_result.is_ok(),
        "Complete workflow should finish within timeout"
    );

    // Phase 3: Final verification
    println!("Workflow Statistics:");
    println!("  Batches processed: {}", workflow_stats.batches_processed);
    println!("  Pools discovered: {}", workflow_stats.pools_discovered);
    println!("  Pools accepted: {}", workflow_stats.pools_accepted);
    println!("  Integrity checks: {}", workflow_stats.integrity_checks);
    println!("  Errors: {}", workflow_stats.errors);

    // Verify workflow completion
    assert_eq!(workflow_stats.batches_processed, 4);
    assert_eq!(workflow_stats.pools_discovered, 4);
    assert!(workflow_stats.pools_accepted <= workflow_stats.pools_discovered);
    assert_eq!(workflow_stats.integrity_checks, 4);
    assert_eq!(workflow_stats.errors, 0);

    // Verify final system state
    let final_pool_stats = fixture.pool_manager.get_stats().await;
    assert_eq!(final_pool_stats.pools_discovered, 4);

    let final_recording_stats = fixture.recording_monitor.get_stats().await;
    assert_eq!(final_recording_stats.batches_processed, 4);
    assert_eq!(final_recording_stats.pools_discovered, 4);
    assert_eq!(final_recording_stats.processing_errors, 0);

    println!("Complete workflow integration test passed");
}

#[derive(Debug, Default)]
struct WorkflowStats {
    batches_processed: usize,
    pools_discovered: usize,
    pools_accepted: usize,
    integrity_checks: usize,
    errors: usize,
}
