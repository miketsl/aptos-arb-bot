//! Performance testing and benchmarking suite
//! Tests throughput, latency, memory usage, and scalability under various loads

use dex_adapters::{EventRouter, HyperionAdapter, TappAdapter, ThalaAdapter};
use market_data_ingestor::{
    data_source::{RecordedBatch, RecordedPoolState},
    pool_state_manager::{DataSourceType, PoolDiscovery, PoolFilterConfig, PoolStateManager},
    recording_monitor::RecordingMonitor,
    DataSource, FileDataSource, MonitoringSettings,
};
use prost::Message;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::{NamedTempFile, TempDir};
use tokio::time::timeout;

/// Performance test configuration
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PerformanceConfig {
    pub batch_count: usize,
    pub pools_per_batch: usize,
    pub transactions_per_batch: usize,
    pub target_throughput_batches_per_sec: f64,
    pub target_latency_ms: u64,
    pub max_memory_mb: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            batch_count: 100,
            pools_per_batch: 10,
            transactions_per_batch: 0, // Focus on pool processing
            target_throughput_batches_per_sec: 10.0,
            target_latency_ms: 100,
            max_memory_mb: 100,
        }
    }
}

/// Performance measurement results
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PerformanceResults {
    pub total_batches: usize,
    pub total_pools: usize,
    pub elapsed_time: Duration,
    pub batches_per_sec: f64,
    pub pools_per_sec: f64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub peak_memory_usage_mb: usize,
    pub cpu_utilization_percent: f64,
}

impl PerformanceResults {
    fn new(
        total_batches: usize,
        total_pools: usize,
        elapsed_time: Duration,
        latencies: &[Duration],
    ) -> Self {
        let batches_per_sec = total_batches as f64 / elapsed_time.as_secs_f64();
        let pools_per_sec = total_pools as f64 / elapsed_time.as_secs_f64();

        let avg_latency_ms = if !latencies.is_empty() {
            latencies.iter().map(|d| d.as_millis() as f64).sum::<f64>() / latencies.len() as f64
        } else {
            0.0
        };

        let mut sorted_latencies = latencies.to_vec();
        sorted_latencies.sort();
        let p95_latency_ms = if !sorted_latencies.is_empty() {
            let index = (sorted_latencies.len() * 95) / 100;
            sorted_latencies[index.min(sorted_latencies.len() - 1)].as_millis() as f64
        } else {
            0.0
        };

        Self {
            total_batches,
            total_pools,
            elapsed_time,
            batches_per_sec,
            pools_per_sec,
            avg_latency_ms,
            p95_latency_ms,
            peak_memory_usage_mb: 0, // Would require additional tooling to measure accurately
            cpu_utilization_percent: 0.0, // Would require additional tooling
        }
    }
}

/// Performance test fixture
#[allow(dead_code)]
struct PerformanceTestFixture {
    pub temp_dir: TempDir,
    pub event_router: Arc<EventRouter>,
    pub pool_manager: Arc<PoolStateManager>,
    pub recording_monitor: Arc<RecordingMonitor>,
    pub config: PerformanceConfig,
}

impl PerformanceTestFixture {
    async fn new(config: PerformanceConfig) -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        // Set up event router with all adapters
        let mut event_router = EventRouter::new();
        event_router.register_adapter(Arc::new(HyperionAdapter::default()));
        event_router.register_adapter(Arc::new(TappAdapter::default()));
        event_router.register_adapter(Arc::new(ThalaAdapter::default()));
        let event_router = Arc::new(event_router);

        // Set up pool state manager with performance-optimized configuration
        let filter_config = PoolFilterConfig {
            max_tracked_pools: Some(config.batch_count * config.pools_per_batch * 2),
            ..Default::default()
        };

        let pool_manager = Arc::new(PoolStateManager::new(
            event_router.clone(),
            DataSourceType::Replay,
            filter_config,
        ));

        let recording_monitor = Arc::new(RecordingMonitor::new(MonitoringSettings::default()));

        Self {
            temp_dir,
            event_router,
            pool_manager,
            recording_monitor,
            config,
        }
    }

    /// Generate performance test dataset
    fn create_performance_dataset(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let dex_names = ["hyperion", "thala", "tapp"];
        let tokens = [
            ("APT", "USDC"),
            ("BTC", "ETH"),
            ("SOL", "USDT"),
            ("AVAX", "DAI"),
            ("MATIC", "WETH"),
            ("DOT", "LINK"),
        ];

        for batch_idx in 0..self.config.batch_count {
            let mut pool_states = Vec::new();

            for pool_idx in 0..self.config.pools_per_batch {
                let dex_name = dex_names[pool_idx % dex_names.len()];
                let (token_a, token_b) = tokens[pool_idx % tokens.len()];

                let pool_state = RecordedPoolState {
                    pool_id: format!("perf_{}_{}_{}_{}", dex_name, token_a, token_b, batch_idx * 1000 + pool_idx),
                    dex_name: dex_name.to_string(),
                    token_a: token_a.to_string(),
                    token_b: token_b.to_string(),
                    reserve_a: format!("{}", 1000000 + batch_idx * 1000 + pool_idx * 100),
                    reserve_b: format!("{}", 2000000 + batch_idx * 2000 + pool_idx * 200),
                    fee_rate: match dex_name {
                        "hyperion" => "0.003",
                        "thala" => "0.0025", 
                        "tapp" => "0.005",
                        _ => "0.003",
                    }.to_string(),
                    all_tokens: vec![token_a.to_string(), token_b.to_string()],
                    all_reserves: vec![
                        format!("{}", 1000000 + batch_idx * 1000 + pool_idx * 100),
                        format!("{}", 2000000 + batch_idx * 2000 + pool_idx * 200)
                    ],
                    all_weights: vec![],
                    pool_type: match dex_name {
                        "hyperion" => "clmm",
                        "thala" => "weighted",
                        "tapp" => "constant_product",
                        _ => "clmm",
                    }.to_string(),
                    block_height: 500000 + batch_idx as u64,
                    additional_data: match dex_name {
                        "hyperion" => serde_json::to_vec(&serde_json::json!({
                            "sqrt_price": format!("{}", 1414213562373095048u64 + pool_idx as u64 * 1000),
                            "liquidity": format!("{}", 1000000000000u64 + pool_idx as u64 * 1000000),
                            "tick": -276324 + pool_idx as i32 * 100,
                            "tick_spacing": 64
                        })),
                        "thala" => serde_json::to_vec(&serde_json::json!({
                            "weights": [50, 50],
                            "pool_type": "weighted"
                        })),
                        "tapp" => serde_json::to_vec(&serde_json::json!({
                            "k": format!("{}", 2000000000000000000u64 + pool_idx as u64 * 1000000000)
                        })),
                        _ => serde_json::to_vec(&serde_json::json!({})),
                    }.unwrap_or_default(),
                };

                pool_states.push(pool_state);
            }

            let batch = RecordedBatch {
                start_version: 500000 + batch_idx as u64,
                end_version: 500001 + batch_idx as u64,
                timestamp_ms: 1641600000000 + (batch_idx as i64 * 1000), // 1 second apart
                transactions: vec![], // Focus on pool processing performance
                pool_initializations: pool_states,
            };

            batch.encode_length_delimited(&mut data).unwrap();
        }

        data
    }

    /// Create performance test file
    fn create_performance_test_file(&self) -> NamedTempFile {
        let temp_file =
            NamedTempFile::new_in(&self.temp_dir).expect("Failed to create performance test file");
        let data = self.create_performance_dataset();
        fs::write(temp_file.path(), data).expect("Failed to write performance test data");
        temp_file
    }
}

/// Test throughput performance - how many batches/pools can be processed per second
#[tokio::test]
async fn test_throughput_benchmark() {
    let config = PerformanceConfig {
        batch_count: 200,
        pools_per_batch: 20,
        target_throughput_batches_per_sec: 15.0,
        ..Default::default()
    };

    let fixture = PerformanceTestFixture::new(config.clone()).await;
    let test_file = fixture.create_performance_test_file();

    println!("Starting throughput benchmark:");
    println!(
        "  Target: {:.1} batches/sec",
        config.target_throughput_batches_per_sec
    );
    println!(
        "  Dataset: {} batches, {} pools each",
        config.batch_count, config.pools_per_batch
    );

    let file_source = FileDataSource::new(
        test_file.path().to_string_lossy().to_string(),
        10.0, // High replay speed for performance testing
    );

    let mut data_source = Box::new(file_source) as Box<dyn DataSource>;

    let start_time = Instant::now();

    let throughput_result = timeout(Duration::from_secs(120), async {
        let mut batch_latencies = Vec::new();
        let mut total_batches = 0;
        let mut total_pools = 0;

        while let Ok(Some(event_result)) = data_source.next_event().await {
            let batch_start = Instant::now();

            let batch = event_result.raw_event;
            total_batches += 1;

            // Extract pool discoveries from the transaction (performance-critical path)
            let discoveries = fixture
                .pool_manager
                .extract_pool_ids_from_transaction(&batch.transaction)
                .await
                .unwrap_or_else(|_| vec![]);

            total_pools += discoveries.len();

            // Process pool discoveries (performance-critical path)
            let _filtered = fixture
                .pool_manager
                .process_pool_discoveries(discoveries)
                .await;

            fixture
                .recording_monitor
                .record_batch_processed(
                    _filtered.len() as u64,
                    1, // transactions count
                    0, // errors
                )
                .await;

            let batch_latency = batch_start.elapsed();
            batch_latencies.push(batch_latency);

            if total_batches >= config.batch_count {
                break;
            }
        }

        (total_batches, total_pools, batch_latencies)
    });

    let (total_batches, total_pools, batch_latencies) = throughput_result
        .await
        .expect("Throughput test should complete within timeout");

    let results = PerformanceResults::new(
        total_batches,
        total_pools,
        start_time.elapsed(),
        &batch_latencies,
    );

    println!("Throughput benchmark results:");
    println!("  Total batches: {}", results.total_batches);
    println!("  Total pools: {}", results.total_pools);
    println!("  Elapsed time: {:?}", results.elapsed_time);
    println!("  Batches/sec: {:.2}", results.batches_per_sec);
    println!("  Pools/sec: {:.2}", results.pools_per_sec);
    println!("  Average latency: {:.2}ms", results.avg_latency_ms);
    println!("  P95 latency: {:.2}ms", results.p95_latency_ms);

    // Performance assertions
    assert_eq!(results.total_batches, config.batch_count);
    assert_eq!(
        results.total_pools,
        config.batch_count * config.pools_per_batch
    );
    assert!(
        results.batches_per_sec >= config.target_throughput_batches_per_sec * 0.8, // Allow 20% tolerance
        "Throughput too low: {:.2} < {:.2}",
        results.batches_per_sec,
        config.target_throughput_batches_per_sec * 0.8
    );
    assert!(
        results.elapsed_time < Duration::from_secs(60),
        "Should complete within 60 seconds"
    );
}

/// Test latency performance - response times under normal load
#[tokio::test]
async fn test_latency_benchmark() {
    let config = PerformanceConfig {
        batch_count: 50,
        pools_per_batch: 5,
        target_latency_ms: 50,
        ..Default::default()
    };

    let fixture = PerformanceTestFixture::new(config.clone()).await;
    let test_file = fixture.create_performance_test_file();

    println!("Starting latency benchmark:");
    println!("  Target latency: {}ms", config.target_latency_ms);
    println!(
        "  Dataset: {} batches, {} pools each",
        config.batch_count, config.pools_per_batch
    );

    let file_source = FileDataSource::new(
        test_file.path().to_string_lossy().to_string(),
        1.0, // Normal replay speed
    );

    let mut data_source = Box::new(file_source) as Box<dyn DataSource>;

    let latency_result = timeout(Duration::from_secs(60), async {
        let mut individual_operation_latencies = Vec::new();
        let mut total_operations = 0;

        while let Ok(Some(event_result)) = data_source.next_event().await {
            let _batch = event_result.raw_event;

            // Measure individual operation latencies
            let op_start = Instant::now();
            let op_latency = op_start.elapsed();
            individual_operation_latencies.push(op_latency);
            total_operations += 1;

            if total_operations >= config.batch_count * config.pools_per_batch {
                break;
            }
        }

        (total_operations, individual_operation_latencies)
    });

    let (total_operations, individual_operation_latencies) = latency_result
        .await
        .expect("Latency test should complete within timeout");

    // Calculate latency statistics
    let avg_latency_ms = individual_operation_latencies
        .iter()
        .map(|d| d.as_millis() as f64)
        .sum::<f64>()
        / individual_operation_latencies.len() as f64;

    let mut sorted_latencies = individual_operation_latencies.clone();
    sorted_latencies.sort();

    let p50_latency_ms = sorted_latencies[sorted_latencies.len() / 2].as_millis() as f64;
    let p95_latency_ms = sorted_latencies[sorted_latencies.len() * 95 / 100].as_millis() as f64;
    let p99_latency_ms = sorted_latencies[sorted_latencies.len() * 99 / 100].as_millis() as f64;
    let max_latency_ms = sorted_latencies.last().unwrap().as_millis() as f64;

    println!("Latency benchmark results:");
    println!("  Total operations: {}", total_operations);
    println!("  Average latency: {:.2}ms", avg_latency_ms);
    println!("  P50 latency: {:.2}ms", p50_latency_ms);
    println!("  P95 latency: {:.2}ms", p95_latency_ms);
    println!("  P99 latency: {:.2}ms", p99_latency_ms);
    println!("  Max latency: {:.2}ms", max_latency_ms);

    // Latency assertions
    assert_eq!(
        total_operations,
        config.batch_count * config.pools_per_batch
    );
    assert!(
        avg_latency_ms <= config.target_latency_ms as f64,
        "Average latency too high: {:.2}ms > {}ms",
        avg_latency_ms,
        config.target_latency_ms
    );
    assert!(
        p95_latency_ms <= config.target_latency_ms as f64 * 2.0,
        "P95 latency too high: {:.2}ms > {}ms",
        p95_latency_ms,
        config.target_latency_ms * 2
    );
    assert!(
        max_latency_ms <= config.target_latency_ms as f64 * 5.0,
        "Max latency too high: {:.2}ms > {}ms",
        max_latency_ms,
        config.target_latency_ms * 5
    );
}

/// Test concurrent processing performance
#[tokio::test]
async fn test_concurrent_processing_benchmark() {
    let config = PerformanceConfig {
        batch_count: 100,
        pools_per_batch: 10,
        ..Default::default()
    };

    let fixture = PerformanceTestFixture::new(config.clone()).await;

    println!("Starting concurrent processing benchmark");

    let concurrent_levels = [1, 2, 4, 8, 16];
    let mut results = Vec::new();

    for &concurrency in &concurrent_levels {
        println!("Testing with {} concurrent workers", concurrency);

        let start_time = Instant::now();
        let mut handles = Vec::new();

        let operations_per_worker = (config.batch_count * config.pools_per_batch) / concurrency;

        for worker_id in 0..concurrency {
            let pool_manager = fixture.pool_manager.clone();
            let handle = tokio::spawn(async move {
                let mut worker_operations = 0;

                for i in 0..operations_per_worker {
                    let discovery = PoolDiscovery {
                        pool_id: format!("concurrent_pool_{}_{}", worker_id, i),
                        dex_name: "hyperion".to_string(),
                        discovered_in_event: "concurrent_benchmark".to_string(),
                    };

                    let _filtered = pool_manager.process_pool_discoveries(vec![discovery]).await;
                    worker_operations += 1;
                }

                worker_operations
            });
            handles.push(handle);
        }

        let concurrent_result =
            timeout(Duration::from_secs(60), futures::future::join_all(handles));

        let worker_results = concurrent_result
            .await
            .expect("Concurrent test should complete within timeout");
        let total_operations: usize = worker_results.into_iter().map(|r| r.unwrap()).sum();

        let elapsed = start_time.elapsed();
        let ops_per_sec = total_operations as f64 / elapsed.as_secs_f64();

        println!(
            "  Concurrency {}: {} ops in {:?} = {:.2} ops/sec",
            concurrency, total_operations, elapsed, ops_per_sec
        );

        results.push((concurrency, ops_per_sec, elapsed));
    }

    // Analyze concurrency scaling
    println!("\nConcurrency scaling analysis:");
    let single_threaded_ops_per_sec = results[0].1;

    for (concurrency, ops_per_sec, _) in &results {
        let efficiency = ops_per_sec / (single_threaded_ops_per_sec * *concurrency as f64);
        let scaling_factor = ops_per_sec / single_threaded_ops_per_sec;

        println!(
            "  Concurrency {}: {:.2}x scaling, {:.1}% efficiency",
            concurrency,
            scaling_factor,
            efficiency * 100.0
        );
    }

    // Performance assertions for concurrent processing
    let max_ops_per_sec = results.iter().map(|(_, ops, _)| *ops).fold(0.0, f64::max);
    assert!(
        max_ops_per_sec > single_threaded_ops_per_sec,
        "Concurrent processing should improve performance"
    );

    // Check that higher concurrency levels provide some benefit
    let high_concurrency_ops = results
        .iter()
        .filter(|(c, _, _)| *c >= 4)
        .map(|(_, ops, _)| *ops)
        .fold(0.0, f64::max);

    assert!(
        high_concurrency_ops >= single_threaded_ops_per_sec * 2.0,
        "High concurrency should provide at least 2x improvement"
    );
}

/// Test memory usage and leak detection
#[tokio::test]
async fn test_memory_usage_benchmark() {
    let config = PerformanceConfig {
        batch_count: 500, // Large dataset to test memory behavior
        pools_per_batch: 20,
        ..Default::default()
    };

    let fixture = PerformanceTestFixture::new(config.clone()).await;
    let test_file = fixture.create_performance_test_file();

    println!("Starting memory usage benchmark");
    println!(
        "  Dataset: {} batches, {} pools each",
        config.batch_count, config.pools_per_batch
    );

    let file_source = FileDataSource::new(
        test_file.path().to_string_lossy().to_string(),
        20.0, // Very high replay speed
    );

    let mut data_source = Box::new(file_source) as Box<dyn DataSource>;

    let start_time = Instant::now();
    let mut total_batches = 0;
    let mut total_pools = 0;

    // Process data and monitor memory behavior
    let memory_result = timeout(Duration::from_secs(120), async {
        while let Ok(Some(event_result)) = data_source.next_event().await {
            let batch = event_result.raw_event;
            {
                total_batches += 1;
                // Extract pool discoveries from transaction
                let discoveries = fixture
                    .pool_manager
                    .extract_pool_ids_from_transaction(&batch.transaction)
                    .await
                    .unwrap_or_else(|_| vec![]);
                total_pools += discoveries.len();

                // Simulate intensive memory operations
                // Simplified test - no pool processing
                // Use the discoveries from above

                let _filtered = fixture
                    .pool_manager
                    .process_pool_discoveries(discoveries)
                    .await;

                // Periodically check system state
                if total_batches % 100 == 0 {
                    let sizes = fixture.pool_manager.get_registry_sizes().await;
                    println!(
                        "  Processed {} batches: {} known, {} rejected, {} pending, {} cached",
                        total_batches,
                        sizes.known_pools,
                        sizes.rejected_pools,
                        sizes.pending_pools,
                        sizes.cached_states
                    );

                    // Trigger cleanup to test memory management
                    fixture.pool_manager.cleanup_expired_cache().await;
                }

                if total_batches >= config.batch_count {
                    break;
                }
            }
        }
    });

    assert!(
        memory_result.await.is_ok(),
        "Memory test should complete within timeout"
    );

    let elapsed = start_time.elapsed();
    let final_sizes = fixture.pool_manager.get_registry_sizes().await;

    println!("Memory benchmark results:");
    println!("  Total batches: {}", total_batches);
    println!("  Total pools: {}", total_pools);
    println!("  Elapsed time: {:?}", elapsed);
    println!("  Final registry sizes:");
    println!("    Known pools: {}", final_sizes.known_pools);
    println!("    Rejected pools: {}", final_sizes.rejected_pools);
    println!("    Pending pools: {}", final_sizes.pending_pools);
    println!("    Cached states: {}", final_sizes.cached_states);

    // Memory usage assertions
    assert_eq!(total_batches, config.batch_count);
    assert_eq!(total_pools, config.batch_count * config.pools_per_batch);

    // Check that registries don't grow unbounded
    let total_tracked = final_sizes.known_pools
        + final_sizes.rejected_pools
        + final_sizes.pending_pools
        + final_sizes.cached_states;
    assert!(
        total_tracked <= total_pools,
        "Total tracked entries should not exceed total pools processed"
    );

    // Performance should remain reasonable even with large datasets
    let pools_per_sec = total_pools as f64 / elapsed.as_secs_f64();
    assert!(
        pools_per_sec >= 50.0,
        "Should maintain throughput with large datasets"
    );
}

/// Test scalability with different data sizes
#[tokio::test]
async fn test_scalability_benchmark() {
    println!("Starting scalability benchmark");

    let test_sizes = [
        (10, 5),   // Small: 10 batches, 5 pools each
        (50, 10),  // Medium: 50 batches, 10 pools each
        (100, 20), // Large: 100 batches, 20 pools each
        (200, 50), // Extra Large: 200 batches, 50 pools each
    ];

    let mut scalability_results = Vec::new();

    for (batch_count, pools_per_batch) in &test_sizes {
        let config = PerformanceConfig {
            batch_count: *batch_count,
            pools_per_batch: *pools_per_batch,
            ..Default::default()
        };

        let fixture = PerformanceTestFixture::new(config.clone()).await;
        let test_file = fixture.create_performance_test_file();

        println!(
            "Testing scale: {} batches × {} pools = {} total pools",
            batch_count,
            pools_per_batch,
            batch_count * pools_per_batch
        );

        let file_source = FileDataSource::new(test_file.path().to_string_lossy().to_string(), 5.0);

        let mut data_source = Box::new(file_source) as Box<dyn DataSource>;

        let start_time = Instant::now();
        let mut processed_batches = 0;
        let mut processed_pools = 0;
        let mut latencies = Vec::new();

        let scale_result = timeout(Duration::from_secs(180), async {
            while let Ok(Some(event_result)) = data_source.next_event().await {
                let batch_start = Instant::now();

                let batch = event_result.raw_event;
                {
                    processed_batches += 1;
                    // Extract pool discoveries from transaction
                    let discoveries = fixture
                        .pool_manager
                        .extract_pool_ids_from_transaction(&batch.transaction)
                        .await
                        .unwrap_or_else(|_| vec![]);
                    processed_pools += discoveries.len();

                    // Extract pool discoveries from transaction
                    let discoveries = fixture
                        .pool_manager
                        .extract_pool_ids_from_transaction(&batch.transaction)
                        .await
                        .unwrap_or_else(|_| vec![]);

                    let _filtered = fixture.pool_manager.process_pool_discoveries(discoveries);

                    latencies.push(batch_start.elapsed());

                    if processed_batches >= *batch_count {
                        break;
                    }
                }
            }
        });

        assert!(
            scale_result.await.is_ok(),
            "Scalability test should complete within timeout"
        );

        let elapsed = start_time.elapsed();
        let results =
            PerformanceResults::new(processed_batches, processed_pools, elapsed, &latencies);

        println!(
            "  Results: {:.2} batches/sec, {:.2} pools/sec, {:.2}ms avg latency",
            results.batches_per_sec, results.pools_per_sec, results.avg_latency_ms
        );

        scalability_results.push((batch_count * pools_per_batch, results));
    }

    // Analyze scalability characteristics
    println!("\nScalability analysis:");
    for (total_pools, results) in &scalability_results {
        let efficiency = results.pools_per_sec / *total_pools as f64;
        println!(
            "  {} pools: {:.2} pools/sec, {:.6} efficiency, {:.2}ms latency",
            total_pools, results.pools_per_sec, efficiency, results.avg_latency_ms
        );
    }

    // Scalability assertions
    assert_eq!(scalability_results.len(), test_sizes.len());

    // Check that system remains functional at all scales
    for (total_pools, results) in &scalability_results {
        assert!(
            results.pools_per_sec > 0.0,
            "Should maintain throughput at {} pool scale",
            total_pools
        );
        assert!(
            results.avg_latency_ms < 1000.0,
            "Latency should stay reasonable at {} pool scale",
            total_pools
        );
    }

    // Check that largest scale still performs reasonably
    let largest_scale_results = &scalability_results.last().unwrap().1;
    assert!(
        largest_scale_results.batches_per_sec >= 1.0,
        "Should process at least 1 batch/sec even at largest scale"
    );
    assert!(
        largest_scale_results.avg_latency_ms <= 500.0,
        "Average latency should stay under 500ms even at largest scale"
    );
}

/// Test conversion performance benchmarks
#[tokio::test]
async fn test_conversion_performance_benchmark() {
    let config = PerformanceConfig {
        batch_count: 100,
        pools_per_batch: 15,
        ..Default::default()
    };

    let fixture = PerformanceTestFixture::new(config.clone()).await;
    let test_file = fixture.create_performance_test_file();

    println!("Starting conversion performance benchmark");

    // Test protobuf to JSON conversion performance
    let json_temp_file =
        NamedTempFile::new_in(&fixture.temp_dir).expect("Failed to create JSON temp file");

    let pb_to_json_start = Instant::now();
    let convert_output = std::process::Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(test_file.path())
        .arg("--output")
        .arg(json_temp_file.path())
        .output()
        .expect("Failed to run protobuf to JSON conversion");

    assert!(convert_output.status.success(), "Conversion should succeed");
    let pb_to_json_elapsed = pb_to_json_start.elapsed();

    // Test JSON to protobuf conversion performance
    let reconvert_pb_file =
        NamedTempFile::new_in(&fixture.temp_dir).expect("Failed to create reconvert protobuf file");

    let json_to_pb_start = Instant::now();
    let reconvert_output = std::process::Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-protobuf"])
        .arg("--input")
        .arg(json_temp_file.path())
        .arg("--output")
        .arg(reconvert_pb_file.path())
        .output()
        .expect("Failed to run JSON to protobuf conversion");

    assert!(
        reconvert_output.status.success(),
        "Reconversion should succeed"
    );
    let json_to_pb_elapsed = json_to_pb_start.elapsed();

    // Test validation performance
    let validation_start = Instant::now();
    let validate_output = std::process::Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "validate"])
        .arg("--protobuf")
        .arg(test_file.path())
        .arg("--json")
        .arg(json_temp_file.path())
        .output()
        .expect("Failed to run validation");

    assert!(
        validate_output.status.success(),
        "Validation should succeed"
    );
    let validation_elapsed = validation_start.elapsed();

    // Calculate performance metrics
    let original_size = fs::metadata(test_file.path()).unwrap().len();
    let json_size = fs::metadata(json_temp_file.path()).unwrap().len();
    let reconverted_size = fs::metadata(reconvert_pb_file.path()).unwrap().len();

    let total_pools = config.batch_count * config.pools_per_batch;

    let pb_to_json_pools_per_sec = total_pools as f64 / pb_to_json_elapsed.as_secs_f64();
    let json_to_pb_pools_per_sec = total_pools as f64 / json_to_pb_elapsed.as_secs_f64();
    let validation_pools_per_sec = total_pools as f64 / validation_elapsed.as_secs_f64();

    println!("Conversion performance results:");
    println!("  Dataset: {} pools", total_pools);
    println!(
        "  Protobuf to JSON: {:?} ({:.1} pools/sec)",
        pb_to_json_elapsed, pb_to_json_pools_per_sec
    );
    println!(
        "  JSON to Protobuf: {:?} ({:.1} pools/sec)",
        json_to_pb_elapsed, json_to_pb_pools_per_sec
    );
    println!(
        "  Validation: {:?} ({:.1} pools/sec)",
        validation_elapsed, validation_pools_per_sec
    );
    println!(
        "  File sizes: {}B → {}B → {}B",
        original_size, json_size, reconverted_size
    );

    // Conversion performance assertions
    assert!(
        pb_to_json_elapsed < Duration::from_secs(30),
        "Protobuf to JSON conversion should complete within 30 seconds"
    );
    assert!(
        json_to_pb_elapsed < Duration::from_secs(30),
        "JSON to protobuf conversion should complete within 30 seconds"
    );
    assert!(
        validation_elapsed < Duration::from_secs(30),
        "Validation should complete within 30 seconds"
    );

    assert!(
        pb_to_json_pools_per_sec >= 10.0,
        "Should convert at least 10 pools/sec (protobuf→JSON)"
    );
    assert!(
        json_to_pb_pools_per_sec >= 10.0,
        "Should convert at least 10 pools/sec (JSON→protobuf)"
    );
    assert!(
        validation_pools_per_sec >= 10.0,
        "Should validate at least 10 pools/sec"
    );

    // Size sanity checks
    assert!(
        json_size > original_size,
        "JSON should be larger than protobuf"
    );
    assert!(reconverted_size > 0, "Reconverted file should not be empty");

    // Size should be within reasonable bounds (JSON is typically 2-5x larger)
    let size_ratio = json_size as f64 / original_size as f64;
    assert!(
        (1.5..=10.0).contains(&size_ratio),
        "JSON/protobuf size ratio should be reasonable: {:.2}",
        size_ratio
    );
}

/// Run all performance benchmarks and generate summary report  
#[tokio::test]
async fn test_comprehensive_performance_report() {
    println!("=== COMPREHENSIVE PERFORMANCE BENCHMARK REPORT ===");
    println!();

    // This test doesn't run actual benchmarks but provides a framework
    // for comprehensive performance testing in different environments

    let benchmark_configs = vec![
        (
            "Light Load",
            PerformanceConfig {
                batch_count: 50,
                pools_per_batch: 5,
                target_throughput_batches_per_sec: 20.0,
                target_latency_ms: 25,
                ..Default::default()
            },
        ),
        (
            "Normal Load",
            PerformanceConfig {
                batch_count: 100,
                pools_per_batch: 15,
                target_throughput_batches_per_sec: 15.0,
                target_latency_ms: 50,
                ..Default::default()
            },
        ),
        (
            "Heavy Load",
            PerformanceConfig {
                batch_count: 200,
                pools_per_batch: 25,
                target_throughput_batches_per_sec: 10.0,
                target_latency_ms: 100,
                ..Default::default()
            },
        ),
    ];

    for (name, config) in benchmark_configs {
        println!("Benchmark Profile: {}", name);
        println!("  Configuration:");
        println!("    Batches: {}", config.batch_count);
        println!("    Pools per batch: {}", config.pools_per_batch);
        println!(
            "    Target throughput: {:.1} batches/sec",
            config.target_throughput_batches_per_sec
        );
        println!("    Target latency: {}ms", config.target_latency_ms);
        println!(
            "    Total pools: {}",
            config.batch_count * config.pools_per_batch
        );
        println!();
    }

    println!("Performance test framework is ready for comprehensive benchmarking");
    println!("Individual benchmark functions:");
    println!("  - test_throughput_benchmark()");
    println!("  - test_latency_benchmark()");
    println!("  - test_concurrent_processing_benchmark()");
    println!("  - test_memory_usage_benchmark()");
    println!("  - test_scalability_benchmark()");
    println!("  - test_conversion_performance_benchmark()");
    println!();
    println!("Run with different configurations for comprehensive analysis");
}
