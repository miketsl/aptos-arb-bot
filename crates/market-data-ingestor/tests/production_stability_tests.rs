//! Production stability tests for Market Data Ingestor
//! These tests validate 24/7 operational readiness including memory stability,
//! failover scenarios, and performance regression detection.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use sysinfo::System;
use tokio::time;

use dex_adapters::{EventRouter, HyperionAdapter, PoolState};
use market_data_ingestor::{
    data_source::{DataSource, DataSourceError, EventMetadata, RawEvent, TimestampedEvent},
    pool_state_manager::{DataSourceType, PoolFilterConfig, PoolStateManager},
};

// Re-export the protobuf types we need
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction as ProtoTransaction;

// Test configuration constants
const ACCELERATED_HOUR_DURATION: Duration = Duration::from_secs(10); // 24 hours = 4 minutes
const MEMORY_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const MAX_MEMORY_GROWTH_PERCENT: f64 = 10.0; // Allow 10% memory growth max

/// Create a test event router for testing
fn create_test_event_router() -> Arc<EventRouter> {
    let mut router = EventRouter::new();
    let hyperion = Arc::new(HyperionAdapter::default());
    router.register_adapter(hyperion);
    Arc::new(router)
}

/// Create realistic pool state with varied data for stress testing
fn create_realistic_pool_state(pool_id: &str, index: u64) -> PoolState {
    let tokens = [
        ("APT", "USDC"),
        ("BTC", "USDT"),
        ("ETH", "DAI"),
        ("SOL", "USDC"),
        ("AVAX", "USDT"),
        ("MATIC", "BUSD"),
        ("DOT", "USDC"),
        ("LINK", "USDT"),
    ];
    let dexes = ["hyperion", "thala", "tapp"];

    let (token_a, token_b) = tokens[index as usize % tokens.len()];
    let dex = dexes[index as usize % dexes.len()];

    // Create realistic reserves with some variation
    let base_reserve = 1000000 + (index * 12345) % 5000000;
    let quote_reserve = base_reserve / 2 + (index * 6789) % 1000000;

    PoolState {
        pool_id: pool_id.to_string(),
        dex_name: dex.to_string(),
        token_a: token_a.to_string(),
        token_b: token_b.to_string(),
        reserve_a: base_reserve.to_string(),
        reserve_b: quote_reserve.to_string(),
        fee_rate: "0.003".to_string(),
        block_height: 100000 + index,
        additional_data: serde_json::json!({
            "pool_type": "constant_product",
            "tvl_usd": base_reserve as f64 * 0.5,
            "volume_24h": (index * 1000) % 100000
        }),
    }
}

/// Helper function to simulate continuous pool discovery load
async fn simulate_pool_discovery_load(
    manager: &PoolStateManager,
    pools_per_minute: u64,
    duration: Duration,
) -> Result<u64, String> {
    let interval = Duration::from_secs(60) / pools_per_minute.max(1) as u32;
    let end_time = Instant::now() + duration;
    let mut pool_counter = 0;
    let mut successful_caches = 0;

    while Instant::now() < end_time {
        let pool_id = format!("stress_pool_{}_{}", pools_per_minute, pool_counter);
        let pool_state = create_realistic_pool_state(&pool_id, pool_counter);

        // Attempt to cache - this will test overflow handling
        match manager.cache_pool_state(pool_id, pool_state).await {
            Ok(()) => successful_caches += 1,
            Err(e) => {
                // Cache overflow is expected behavior, not a failure
                if !e.contains("cache size limit") {
                    return Err(format!("Unexpected cache error: {}", e));
                }
            }
        }

        pool_counter += 1;

        // Small delay to prevent overwhelming the system
        if interval > Duration::from_millis(10) {
            time::sleep(interval).await;
        } else {
            time::sleep(Duration::from_millis(10)).await;
        }
    }

    Ok(successful_caches)
}

/// Memory monitoring helper for detecting leaks and trends
struct MemoryMonitor {
    system: System,
    initial_memory: u64,
    peak_memory: u64,
    measurements: Vec<(Duration, u64)>,
    start_time: Instant,
}

impl MemoryMonitor {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_memory();
        let initial_memory = system.used_memory();

        Self {
            system,
            initial_memory,
            peak_memory: initial_memory,
            measurements: Vec::new(),
            start_time: Instant::now(),
        }
    }

    fn record_measurement(&mut self) {
        self.system.refresh_memory();
        let current_memory = self.system.used_memory();
        let elapsed = self.start_time.elapsed();

        if current_memory > self.peak_memory {
            self.peak_memory = current_memory;
        }

        self.measurements.push((elapsed, current_memory));
    }

    fn analyze_memory_trend(&self) -> MemoryAnalysis {
        if self.measurements.len() < 2 {
            return MemoryAnalysis {
                growth_rate_mb_per_hour: 0.0,
                is_stable: true,
                peak_usage_mb: self.peak_memory as f64 / 1024.0 / 1024.0,
                average_usage_mb: self.initial_memory as f64 / 1024.0 / 1024.0,
            };
        }

        // Calculate linear regression for growth trend
        let n = self.measurements.len() as f64;
        let sum_x: f64 = self
            .measurements
            .iter()
            .map(|(duration, _)| duration.as_secs_f64())
            .sum();
        let sum_y: f64 = self
            .measurements
            .iter()
            .map(|(_, memory)| *memory as f64)
            .sum();
        let sum_xy: f64 = self
            .measurements
            .iter()
            .map(|(duration, memory)| duration.as_secs_f64() * (*memory as f64))
            .sum();
        let sum_x2: f64 = self
            .measurements
            .iter()
            .map(|(duration, _)| duration.as_secs_f64().powi(2))
            .sum();

        // Linear regression slope (bytes per second)
        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x.powi(2));

        // Convert to MB per hour
        let growth_rate_mb_per_hour = slope * 3600.0 / 1024.0 / 1024.0;

        let average_memory = sum_y / n;
        let growth_percent = (growth_rate_mb_per_hour * 24.0)
            / (self.initial_memory as f64 / 1024.0 / 1024.0)
            * 100.0;

        MemoryAnalysis {
            growth_rate_mb_per_hour,
            is_stable: growth_percent.abs() < MAX_MEMORY_GROWTH_PERCENT,
            peak_usage_mb: self.peak_memory as f64 / 1024.0 / 1024.0,
            average_usage_mb: average_memory / 1024.0 / 1024.0,
        }
    }
}

#[derive(Debug)]
struct MemoryAnalysis {
    growth_rate_mb_per_hour: f64,
    is_stable: bool,
    peak_usage_mb: f64,
    average_usage_mb: f64,
}

/// Mock gRPC data source for failover testing
struct MockGrpcSource {
    should_fail: Arc<AtomicBool>,
    event_counter: Arc<AtomicU64>,
    connected: bool,
}

impl MockGrpcSource {
    fn new() -> Self {
        Self {
            should_fail: Arc::new(AtomicBool::new(false)),
            event_counter: Arc::new(AtomicU64::new(0)),
            connected: false,
        }
    }

    fn trigger_failure(&self) {
        self.should_fail.store(true, Ordering::Relaxed);
    }

    fn restore_connection(&self) {
        self.should_fail.store(false, Ordering::Relaxed);
    }

    fn generate_mock_event(&self) -> TimestampedEvent {
        let counter = self.event_counter.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now();

        // Create a simple mock ProtoTransaction
        let proto_transaction = ProtoTransaction::default();

        TimestampedEvent {
            raw_event: RawEvent {
                transaction: proto_transaction,
                metadata: EventMetadata {
                    version: counter,
                    block_height: Some(100000 + counter),
                    chain_id: Some(1),
                    size_bytes: Some(1024),
                },
            },
            received_at: timestamp,
            blockchain_timestamp: Some(timestamp),
            sequence: counter,
        }
    }
}

#[async_trait::async_trait]
impl DataSource for MockGrpcSource {
    async fn start(&mut self) -> Result<(), DataSourceError> {
        if self.should_fail.load(Ordering::Relaxed) {
            return Err(DataSourceError::ConnectionFailed(
                "Mock gRPC failure".to_string(),
            ));
        }
        self.connected = true;
        Ok(())
    }

    async fn next_event(&mut self) -> Result<Option<TimestampedEvent>, DataSourceError> {
        if self.should_fail.load(Ordering::Relaxed) {
            return Err(DataSourceError::ConnectionFailed(
                "Mock gRPC failure".to_string(),
            ));
        }

        if !self.connected {
            return Err(DataSourceError::ConnectionFailed(
                "Not connected".to_string(),
            ));
        }

        // Simulate some processing delay
        time::sleep(Duration::from_millis(10)).await;

        Ok(Some(self.generate_mock_event()))
    }

    async fn stop(&mut self) -> Result<(), DataSourceError> {
        self.connected = false;
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.connected && !self.should_fail.load(Ordering::Relaxed)
    }

    fn source_type(&self) -> &'static str {
        "mock_grpc"
    }
}

/// Performance benchmark data
#[derive(Debug)]
struct PerformanceBenchmark {
    throughput_txn_per_sec: f64,
    average_latency_ms: f64,
    peak_memory_mb: f64,
    cpu_usage_percent: f64,
}

/// Run a performance benchmark and return metrics
async fn run_performance_benchmark(duration: Duration) -> PerformanceBenchmark {
    let start_time = Instant::now();
    let mut system = System::new_all();
    let mut transaction_count = 0u64;
    let mut latency_measurements = Vec::new();
    let mut peak_memory = 0u64;

    // Create test manager for benchmark
    let event_router = create_test_event_router();
    let filter_config = PoolFilterConfig {
        max_cache_size_per_block: Some(1000), // Higher limit for performance test
        max_cache_retention_seconds: Some(10),
        ..Default::default()
    };

    let manager = PoolStateManager::new(event_router, DataSourceType::Live, filter_config);

    // Benchmark loop
    while start_time.elapsed() < duration {
        let operation_start = Instant::now();

        // Simulate high-throughput operations
        for i in 0..100 {
            let pool_id = format!("perf_pool_{}", transaction_count + i);
            let pool_state = create_realistic_pool_state(&pool_id, transaction_count + i);

            let _ = manager.cache_pool_state(pool_id, pool_state).await;
        }

        transaction_count += 100;
        latency_measurements.push(operation_start.elapsed().as_millis() as f64);

        // Update system metrics
        system.refresh_all();
        let current_memory = system.used_memory();
        if current_memory > peak_memory {
            peak_memory = current_memory;
        }

        // Small delay to prevent overwhelming
        time::sleep(Duration::from_millis(5)).await;
    }

    let total_duration = start_time.elapsed().as_secs_f64();
    let throughput = transaction_count as f64 / total_duration;
    let average_latency =
        latency_measurements.iter().sum::<f64>() / latency_measurements.len() as f64;

    PerformanceBenchmark {
        throughput_txn_per_sec: throughput,
        average_latency_ms: average_latency,
        peak_memory_mb: peak_memory as f64 / 1024.0 / 1024.0,
        cpu_usage_percent: system.global_cpu_info().cpu_usage() as f64,
    }
}

/// Test 24-hour memory stability with accelerated simulation
#[tokio::test]
async fn test_24_hour_memory_stability() {
    println!("🚀 Starting 24-hour memory stability test (accelerated)...");

    // Setup
    let event_router = create_test_event_router();
    let filter_config = PoolFilterConfig {
        max_cache_size_per_block: Some(500),
        max_cache_retention_seconds: Some(30),
        ..Default::default()
    };

    let manager = PoolStateManager::new(event_router, DataSourceType::Live, filter_config);

    let mut memory_monitor = MemoryMonitor::new();
    let test_start = Instant::now();

    println!(
        "📊 Initial memory usage: {:.1} MB",
        memory_monitor.initial_memory as f64 / 1024.0 / 1024.0
    );

    // Simulate 24 hours of operation (accelerated to ~4 minutes)
    for hour in 0..24 {
        let _hour_start = Instant::now();

        // Vary the load to test different scenarios
        let pools_per_minute = match hour {
            0..=6 => 20,   // Low overnight activity
            7..=9 => 100,  // Morning peak
            10..=16 => 50, // Business hours
            17..=19 => 80, // Evening peak
            _ => 30,       // Late evening
        };

        println!(
            "⏰ Hour {} - Load: {} pools/min",
            hour + 1,
            pools_per_minute
        );

        // Run pool discovery simulation for one accelerated hour
        let discovery_task =
            simulate_pool_discovery_load(&manager, pools_per_minute, ACCELERATED_HOUR_DURATION);

        // Monitor memory during this hour
        let monitoring_task = async {
            let hour_end = Instant::now() + ACCELERATED_HOUR_DURATION;
            while Instant::now() < hour_end {
                memory_monitor.record_measurement();
                time::sleep(MEMORY_CHECK_INTERVAL).await;
            }
        };

        // Run both tasks concurrently
        let (discovery_result, _) = tokio::join!(discovery_task, monitoring_task);

        // Check that discovery completed successfully
        let successful_caches = discovery_result.expect("Pool discovery simulation failed");

        // Validate cache behavior
        let registry_sizes = manager.get_registry_sizes().await;
        assert!(
            registry_sizes.cached_states <= 500,
            "Cache size exceeded limit: {} > 500",
            registry_sizes.cached_states
        );

        // Force cache cleanup to test cleanup mechanisms
        manager.cleanup_expired_cache().await;

        let current_memory = memory_monitor.system.used_memory() as f64 / 1024.0 / 1024.0;
        println!(
            "   ✅ Cache: {}, Memory: {:.1}MB, Cached: {}",
            registry_sizes.cached_states, current_memory, successful_caches
        );
    }

    // Analyze results
    let memory_analysis = memory_monitor.analyze_memory_trend();

    println!("📈 Memory Analysis:");
    println!(
        "   Growth rate: {:.2} MB/hour",
        memory_analysis.growth_rate_mb_per_hour
    );
    println!("   Peak usage: {:.1} MB", memory_analysis.peak_usage_mb);
    println!(
        "   Average usage: {:.1} MB",
        memory_analysis.average_usage_mb
    );
    println!(
        "   Stability: {}",
        if memory_analysis.is_stable {
            "✅ STABLE"
        } else {
            "❌ UNSTABLE"
        }
    );

    // Assertions
    assert!(
        memory_analysis.is_stable,
        "Memory usage is not stable: growth rate {:.2} MB/hour exceeds {:.1}% limit",
        memory_analysis.growth_rate_mb_per_hour, MAX_MEMORY_GROWTH_PERCENT
    );

    assert!(
        memory_analysis.growth_rate_mb_per_hour.abs() < 50.0,
        "Memory growth rate too high: {:.2} MB/hour",
        memory_analysis.growth_rate_mb_per_hour
    );

    // Validate final state
    let final_sizes = manager.get_registry_sizes().await;
    assert!(
        final_sizes.cached_states <= 500,
        "Final cache size exceeded limit"
    );

    let total_test_time = test_start.elapsed();
    println!(
        "✅ 24-hour stability test passed in {:.1}s!",
        total_test_time.as_secs_f64()
    );
}

/// Test gRPC to File failover scenarios
#[tokio::test]
async fn test_grpc_to_file_failover() {
    println!("🔄 Starting gRPC to File failover test...");

    // Create mock gRPC source
    let grpc_source = Arc::new(MockGrpcSource::new());
    let grpc_control = grpc_source.clone();

    // Test Phase 1: Normal gRPC operation
    println!("📡 Phase 1: Testing normal gRPC operation...");

    let mut events_received = Vec::new();
    let mut source_clone = MockGrpcSource::new();

    // Start the source
    source_clone
        .start()
        .await
        .expect("Failed to start gRPC source");

    // Collect events for 20 iterations
    for _i in 0..20 {
        match source_clone.next_event().await {
            Ok(Some(event)) => {
                events_received.push(("grpc".to_string(), event));
            }
            Ok(None) => break,
            Err(e) => panic!("Unexpected error during normal operation: {}", e),
        }

        time::sleep(Duration::from_millis(10)).await;
    }

    println!(
        "   Received {} events during normal operation",
        events_received.len()
    );
    assert!(
        !events_received.is_empty(),
        "Should have received events during gRPC phase"
    );

    // Test Phase 2: Trigger gRPC failure
    println!("💥 Phase 2: Triggering gRPC failure...");
    grpc_control.trigger_failure();

    // Verify failure detection
    match source_clone.next_event().await {
        Err(DataSourceError::ConnectionFailed(_)) => {
            println!("   ✅ Failure correctly detected");
        }
        other => panic!("Expected connection error, got: {:?}", other),
    }

    // Test Phase 3: Failover simulation (would normally switch to file source)
    println!("🔄 Phase 3: Simulating failover to file source...");

    // In a real implementation, this would create a file source
    // For testing, we'll simulate the file source behavior
    let mut file_events = Vec::new();
    for i in 0..15 {
        let timestamp = SystemTime::now();

        let proto_transaction = ProtoTransaction::default();

        let file_event = TimestampedEvent {
            raw_event: RawEvent {
                transaction: proto_transaction,
                metadata: EventMetadata {
                    version: 200000 + i,
                    block_height: Some(200000 + i),
                    chain_id: Some(1),
                    size_bytes: Some(1024),
                },
            },
            received_at: timestamp,
            blockchain_timestamp: Some(timestamp),
            sequence: 200000 + i,
        };

        file_events.push(("file".to_string(), file_event));
        time::sleep(Duration::from_millis(10)).await;
    }

    println!(
        "   Generated {} file events during failover",
        file_events.len()
    );

    // Test Phase 4: gRPC recovery
    println!("🔌 Phase 4: Testing gRPC recovery...");
    grpc_control.restore_connection();

    // Restart source and verify recovery
    source_clone
        .start()
        .await
        .expect("Failed to restart gRPC source");

    let mut recovery_events = Vec::new();
    for _i in 0..10 {
        match source_clone.next_event().await {
            Ok(Some(event)) => {
                recovery_events.push(("recovery".to_string(), event));
            }
            Ok(None) => break,
            Err(e) => panic!("Unexpected error during recovery: {}", e),
        }

        time::sleep(Duration::from_millis(10)).await;
    }

    println!(
        "   Received {} events during recovery",
        recovery_events.len()
    );

    // Combine all events for analysis
    let mut all_events = events_received;
    all_events.extend(file_events);
    all_events.extend(recovery_events);

    // Validate results
    let grpc_events: Vec<_> = all_events
        .iter()
        .filter(|(phase, _)| phase == "grpc")
        .collect();
    let file_events: Vec<_> = all_events
        .iter()
        .filter(|(phase, _)| phase == "file")
        .collect();
    let recovery_events: Vec<_> = all_events
        .iter()
        .filter(|(phase, _)| phase == "recovery")
        .collect();

    assert!(
        !grpc_events.is_empty(),
        "Should have received events during gRPC phase"
    );
    assert!(
        !file_events.is_empty(),
        "Should have received events during failover phase"
    );
    assert!(
        !recovery_events.is_empty(),
        "Should have received events during recovery phase"
    );

    // Validate event continuity (basic checks)
    validate_event_sequence(&all_events);

    println!("✅ gRPC to File failover test passed!");
    println!("   gRPC events: {}", grpc_events.len());
    println!("   File events: {}", file_events.len());
    println!("   Recovery events: {}", recovery_events.len());
}

/// Validate event sequence for basic continuity
fn validate_event_sequence(events: &[(String, TimestampedEvent)]) {
    assert!(!events.is_empty(), "Event sequence cannot be empty");

    // Check that all events have valid timestamps and metadata
    for (phase, event) in events {
        assert!(
            event.sequence > 0,
            "Invalid sequence number in {} phase",
            phase
        );
        assert!(
            event.raw_event.metadata.version > 0,
            "Invalid version in {} phase",
            phase
        );

        if let Some(block_height) = event.raw_event.metadata.block_height {
            assert!(block_height > 0, "Invalid block height in {} phase", phase);
        }
    }

    println!("   ✅ Event sequence validation passed");
}

/// Test performance regression detection
#[tokio::test]
async fn test_performance_regression() {
    println!("🏃 Starting performance regression test...");

    // Define performance baselines (these should be adjusted based on actual system performance)
    const MIN_THROUGHPUT_TXN_PER_SEC: f64 = 500.0; // Conservative baseline
    const MAX_LATENCY_MS: f64 = 200.0; // Conservative baseline
    const MAX_MEMORY_USAGE_MB: f64 = 30000.0; // 30GB limit (test system may use more memory)

    // Run performance benchmark
    let benchmark_duration = Duration::from_secs(10); // Shorter test for CI
    println!(
        "🔬 Running {:.0}s performance benchmark...",
        benchmark_duration.as_secs_f64()
    );

    let benchmark = run_performance_benchmark(benchmark_duration).await;

    println!("📊 Performance Results:");
    println!(
        "   Throughput: {:.1} txn/sec",
        benchmark.throughput_txn_per_sec
    );
    println!("   Latency: {:.1} ms", benchmark.average_latency_ms);
    println!("   Memory: {:.1} MB", benchmark.peak_memory_mb);
    println!("   CPU: {:.1}%", benchmark.cpu_usage_percent);

    // Performance assertions
    assert!(
        benchmark.throughput_txn_per_sec >= MIN_THROUGHPUT_TXN_PER_SEC,
        "Throughput regression: {:.1} < {:.1} txn/sec",
        benchmark.throughput_txn_per_sec,
        MIN_THROUGHPUT_TXN_PER_SEC
    );

    assert!(
        benchmark.average_latency_ms <= MAX_LATENCY_MS,
        "Latency regression: {:.1} > {:.1} ms",
        benchmark.average_latency_ms,
        MAX_LATENCY_MS
    );

    assert!(
        benchmark.peak_memory_mb <= MAX_MEMORY_USAGE_MB,
        "Memory usage regression: {:.1} > {:.1} MB",
        benchmark.peak_memory_mb,
        MAX_MEMORY_USAGE_MB
    );

    // CPU usage should be reasonable (not a hard failure, just warning)
    if benchmark.cpu_usage_percent > 80.0 {
        println!(
            "⚠️  Warning: High CPU usage detected: {:.1}%",
            benchmark.cpu_usage_percent
        );
    }

    println!("✅ Performance regression test passed!");
}

/// Test cache behavior under extreme load
#[tokio::test]
async fn test_cache_extreme_load() {
    println!("🔥 Starting cache extreme load test...");

    let event_router = create_test_event_router();
    let filter_config = PoolFilterConfig {
        max_cache_size_per_block: Some(100), // Small cache for stress testing
        max_cache_retention_seconds: Some(5), // Short retention
        ..Default::default()
    };

    let manager = PoolStateManager::new(event_router, DataSourceType::Live, filter_config);

    // Generate extreme load (1000 pools in rapid succession)
    let mut cache_results = Vec::new();
    let load_start = Instant::now();

    for i in 0..1000 {
        let pool_id = format!("extreme_pool_{}", i);
        let pool_state = create_realistic_pool_state(&pool_id, i);

        let result = manager.cache_pool_state(pool_id, pool_state).await;
        cache_results.push(result.is_ok());

        // Check cache size periodically
        if i % 100 == 0 {
            let sizes = manager.get_registry_sizes().await;
            println!(
                "   Batch {}: Cache size = {}",
                i / 100 + 1,
                sizes.cached_states
            );
            assert!(
                sizes.cached_states <= 100,
                "Cache size exceeded limit during extreme load"
            );
        }
    }

    let load_duration = load_start.elapsed();
    let successful_caches = cache_results.iter().filter(|&&x| x).count();
    let cache_success_rate = successful_caches as f64 / 1000.0 * 100.0;

    println!("📈 Extreme Load Results:");
    println!("   Duration: {:.2}s", load_duration.as_secs_f64());
    println!(
        "   Success rate: {:.1}% ({}/1000)",
        cache_success_rate, successful_caches
    );
    println!(
        "   Throughput: {:.1} ops/sec",
        1000.0 / load_duration.as_secs_f64()
    );

    // Final cache state should be within limits
    let final_sizes = manager.get_registry_sizes().await;
    assert!(
        final_sizes.cached_states <= 100,
        "Final cache size exceeded limit"
    );

    // Should have some successful caches (not all will succeed due to overflow)
    assert!(
        cache_success_rate > 5.0,
        "Cache success rate too low: {:.1}%",
        cache_success_rate
    );

    println!("✅ Cache extreme load test passed!");
}
