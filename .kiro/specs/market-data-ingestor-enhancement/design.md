# Design Document

## Overview

The Market Data Ingestor (MDI) enhancement focuses on transforming the current basic implementation into a production-grade system capable of handling multiple data sources, comprehensive testing workflows, robust error handling, and **cold boot pool state initialization**. The design maintains the existing async pipeline architecture while adding configurable data sources, bidirectional test data tooling, flexible filtering, comprehensive monitoring, and a critical pool state management system.

The enhanced MDI will serve as the critical first stage in the arbitrage detection pipeline, ensuring reliable, low-latency market data delivery to the detector component while solving the **cold boot problem** - the challenge of discovering new pools and initializing their complete state when they first appear in transaction events.

**Key Cold Boot Challenge**: When the MDI processes a transaction containing a pool that isn't in the detector graph, it needs to:
1. Detect that this is a new/unknown pool
2. Fetch the complete current state of that pool via REST API
3. Initialize the detector graph with this pool state
4. Then continue processing incremental updates via the gRPC stream

This hybrid approach (REST for initialization + gRPC for updates) ensures the detector always has complete, accurate pool state.

## Architecture

### High-Level Architecture

```
┌─────────────────┐    ┌──────────────────────────────┐    ┌─────────────────┐
│   Data Sources  │    │         MDI Core             │    │   Downstream    │
│                 │    │                              │    │                 │
│ ┌─────────────┐ │    │ ┌──────────────┐             │    │ ┌─────────────┐ │
│ │ gRPC Stream │ │───▶│ │ Event Router │             │    │ │  Detector   │ │
│ └─────────────┘ │    │ └──────────────┘             │    │ └─────────────┘ │
│ ┌─────────────┐ │    │ ┌──────────────┐             │    │                 │
│ │ File Replay │ │───▶│ │ DEX Adapters │             │    │                 │
│ └─────────────┘ │    │ └──────────────┘             │    │                 │
│ ┌─────────────┐ │    │ ┌──────────────┐             │    │                 │
│ │ REST APIs   │ │───▶│ │Pool Discovery│◄────────────┤    │                 │
│ └─────────────┘ │    │ │& State Mgmt  │             │    │                 │
│                 │    │ └──────────────┘             │    │                 │
│                 │    │ ┌──────────────┐             │    │                 │
│                 │    │ │   Filters    │             │    │                 │
│                 │    │ └──────────────┘             │    │                 │
└─────────────────┘    └──────────────────────────────┘    └─────────────────┘
```

### Component Interaction Flow

1. **Configuration Loading**: System loads YAML configuration specifying data source type and parameters
2. **Data Source Initialization**: Appropriate data source (gRPC or File) is instantiated based on config
3. **Event Processing Pipeline**: Raw events flow through parsing, filtering, and transformation stages
4. **Market Update Delivery**: Standardized MarketUpdate structures are sent to detector via channels

## Components and Interfaces

### Data Source Abstraction

**Design Decision**: Use a trait-based approach for data sources to enable seamless switching between live and recorded data without changing core processing logic.

```rust
#[async_trait]
pub trait DataSource: Send + Sync {
    async fn start(&mut self) -> Result<(), IngestorError>;
    async fn next_event(&mut self) -> Result<Option<RawEvent>, IngestorError>;
    async fn stop(&mut self) -> Result<(), IngestorError>;
}
```

**Rationale**: This abstraction allows identical processing behavior regardless of data source, critical for testing and development workflows.

### gRPC Data Source

Handles live blockchain data streaming with robust connection management:

```rust
pub struct GrpcDataSource {
    client: AptosTxnStreamClient,
    config: GrpcConfig,
    reconnect_strategy: ExponentialBackoff,
}
```

**Key Features**:
- Automatic reconnection with exponential backoff
- Connection health monitoring
- Graceful degradation on temporary failures

### File Data Source

Supports protobuf file replay with configurable timing:

```rust
pub struct FileDataSource {
    file_path: PathBuf,
    replay_speed: Option<f64>, // None = as fast as possible
    current_position: usize,
    events: Vec<TimestampedEvent>,
}
```

**Design Decision**: Store timing metadata with events to enable realistic replay scenarios.

**Rationale**: Developers need to test timing-sensitive arbitrage logic with realistic event spacing.

### Test Data Tooling

**Bidirectional Conversion Pipeline**:

```
Protobuf ←→ JSON ←→ Human Editing ←→ JSON ←→ Protobuf
```

**Components**:
- **Recorder**: Captures live gRPC stream to protobuf files
- **Converter**: Bidirectional protobuf ↔ JSON transformation
- **Validator**: Ensures data integrity during conversion cycles

**Design Decision**: Use JSON as the human-editable intermediate format rather than YAML or TOML.

**Rationale**: JSON provides better tooling support and is more familiar to developers working with blockchain data.

### Enhanced DEX Adapter System

**Generic Adapter Interface**:

Each DEX adapter encapsulates ALL DEX-specific logic including event parsing, pool state fetching, and transaction execution.

```rust
#[async_trait]
pub trait DexAdapter: Send + Sync {
    fn module_addresses(&self) -> &[AccountAddress];
    async fn parse_event(&self, event: &Event) -> Result<Vec<MarketUpdate>, AdapterError>;
    async fn get_pool_state(&self, pool_id: &PoolId) -> Result<PoolState, AdapterError>;
    fn adapter_name(&self) -> &'static str;
}
```

**Design Decision**: Each DEX adapter contains all DEX-specific logic rather than separating concerns across multiple components.

**Rationale**: Different DEXes have unique pool types, REST API endpoints, transaction formats, and business logic. Encapsulating all DEX-specific functionality within each adapter prevents scattering DEX-specific code across the codebase and makes adding new DEXes simpler.

**Event Routing System**:

```rust
pub struct EventRouter {
    adapters: HashMap<AccountAddress, Box<dyn DexAdapter>>,
    fallback_handler: Option<Box<dyn DexAdapter>>,
}
```

**Design Decision**: Route events based on module address rather than event type inspection.

**Rationale**: Module address routing is more efficient and reliable than pattern matching on event structures.

### Filtering System

**Multi-Level Filtering Architecture**:

```rust
pub struct FilterConfig {
    pub token_whitelist: Option<Vec<String>>,
    pub token_pairs: Option<Vec<(String, String)>>,
    pub min_liquidity: Option<Decimal>,
    pub dex_whitelist: Option<Vec<String>>,
}

pub struct MarketUpdateFilter {
    config: FilterConfig,
}
```

**Design Decision**: Apply filters after parsing but before sending to detector.

**Rationale**: This placement reduces detector workload while preserving parsing accuracy for debugging.

### Pool State Management System

**The Cold Boot Problem Solution**:

The Pool State Management system solves the critical cold boot problem by maintaining a registry of known pools and handling pool state initialization differently based on the data source type:

- **Live gRPC streams**: Fetch current pool state via REST APIs when unknown pools are discovered
- **File replay**: Use embedded pool state data from the replay file to maintain historical consistency

```rust
pub struct PoolStateManager {
    known_pools: Arc<RwLock<HashSet<PoolId>>>,
    rejected_pools: Arc<RwLock<HashSet<PoolId>>>,
    pending_pools: Arc<RwLock<HashSet<PoolId>>>,
    new_pool_state_cache: Arc<RwLock<HashMap<PoolId, PoolState>>>,
    adapters: HashMap<String, Box<dyn DexAdapter>>,
    detector_channel: mpsc::Sender<DetectorMessage>,
    data_source_type: DataSourceType,
}

#[derive(Debug, Clone)]
pub enum DataSourceType {
    Live,
    Replay,
}
```

**Pool Discovery Flow - Data Source Specific Behavior**:

The pool discovery system handles unknown pools differently based on the data source type:

### Live gRPC Stream Pool Discovery

**During Event Processing**:
1. **Pool Discovery**: DEX adapter parses event and extracts pool references
2. **Registry Check**: System checks if pool is in `known_pools` or `rejected_pools` registry
3. **Filter Check**: If unknown, apply TVL and other filters to determine if pool is worth tracking
4. **Async State Fetch**: If pool passes filters:
   - Spawn async worker to fetch complete pool state via REST API
   - Store result in `new_pool_state_cache` when complete
   - Add pool ID to `pending_pools` registry to avoid duplicate fetches
5. **Event Dropping**: Drop the current market update event for this newly discovered pool
6. **Continue Processing**: Process other events in the block normally

**Block Processing Flow**:
1. **Block Start**: Send `DetectorMessage::BlockStart` to begin block processing
2. **New Pool Integration**: Check `new_pool_state_cache` for completed pool state fetches:
   - For each completed pool state, send `DetectorMessage::PoolInitialization`
   - Move pool from `pending_pools` to `known_pools` registry
   - Clear processed entries from `new_pool_state_cache`
3. **Market Updates**: Process and send `DetectorMessage::MarketUpdate` for known pools only
4. **Block End**: Send `DetectorMessage::BlockEnd` to trigger strategy execution

### File Replay Pool Discovery

**Critical Design Challenge**: File replay cannot use REST APIs for pool state because:
- Recorded transactions are historical, but REST APIs return current state
- Pool state from "now" doesn't match the historical context of replayed events
- This creates inconsistencies between pool state and market updates

**File Replay Solution**: Embed historical pool state data directly in replay files

**Enhanced RecordedBatch Structure**:
```rust
#[derive(prost::Message, Serialize, Deserialize)]
pub struct RecordedBatch {
    #[prost(uint64, tag = "1")]
    pub start_version: u64,
    #[prost(uint64, tag = "2")]
    pub end_version: u64,
    #[prost(int64, tag = "3")]
    pub timestamp_ms: i64,
    #[prost(message, repeated, tag = "4")]
    pub transactions: Vec<ProtoTransaction>,
    // NEW: Embedded pool states for newly discovered pools in this batch
    #[prost(message, repeated, tag = "5")]
    pub pool_initializations: Vec<RecordedPoolState>,
}

#[derive(prost::Message, Serialize, Deserialize)]
pub struct RecordedPoolState {
    #[prost(string, tag = "1")]
    pub pool_id: String,
    #[prost(string, tag = "2")]
    pub dex_name: String,
    #[prost(string, tag = "3")]
    pub token_a: String,
    #[prost(string, tag = "4")]
    pub token_b: String,
    #[prost(string, tag = "5")]
    pub reserve_a: String, // Decimal as string
    #[prost(string, tag = "6")]
    pub reserve_b: String, // Decimal as string
    #[prost(string, tag = "7")]
    pub fee_rate: String,  // Decimal as string
    #[prost(uint64, tag = "8")]
    pub block_height: u64,
    #[prost(bytes, tag = "9")]
    pub additional_data: Vec<u8>, // JSON serialized DEX-specific data
}
```

**File Replay Pool Discovery Flow**:
1. **Batch Processing**: When processing a RecordedBatch, first check for `pool_initializations`
2. **Pool State Integration**: For each RecordedPoolState:
   - Convert to internal PoolState format
   - Send `DetectorMessage::PoolInitialization` to detector
   - Add pool to `known_pools` registry
3. **Event Processing**: Process transactions normally, knowing all required pool states are already initialized
4. **No REST API Calls**: File replay never makes REST API calls for pool state

**Recording Enhancement**: The recording tool must be enhanced to:
1. **Detect New Pools**: Monitor for unknown pools during live recording
2. **Fetch Pool State**: When a new pool is discovered, immediately fetch its state via REST API
3. **Embed State Data**: Include the fetched pool state in the RecordedBatch for that block
4. **Maintain Consistency**: Ensure pool state timestamp matches the block being recorded

**Critical Design Decision**: Drop initial events for newly discovered pools rather than blocking the pipeline.

**Rationale**: 
- Maintains sub-100ms block processing latency
- REST API provides current pool state that's likely more accurate than reconstructing from dropped events
- Subsequent blocks will contain incremental updates once pool is in `known_pools`
- Async workers can take their time without affecting main pipeline performance

**Design Decision**: Use separate REST API calls for pool state initialization rather than trying to reconstruct state from event history.

**Rationale**: REST APIs provide atomic, consistent pool state snapshots, while event reconstruction would be complex and error-prone.

**DEX Adapter Pool State Integration**:

Each DEX adapter implements the `get_pool_state` method with its own REST API integration:

```rust
impl DexAdapter for HyperionAdapter {
    async fn get_pool_state(&self, pool_id: &PoolId) -> Result<PoolState, AdapterError> {
        let url = format!("{}/pools/{}", self.base_url, pool_id);
        let response = self.client.get(&url).send().await?;
        let pool_data: HyperionPoolResponse = response.json().await?;
        Ok(pool_data.into())
    }
}
```

**Pool State Caching**:

```rust
#[derive(Debug, Clone)]
pub struct CachedPoolState {
    pub state: PoolState,
    pub last_updated: DateTime<Utc>,
    pub ttl: Duration,
}
```

**Design Decision**: Cache pool states with TTL to reduce REST API calls for frequently accessed pools.

**Rationale**: Balances data freshness with API rate limiting and performance.

### Performance Monitoring

**Metrics Collection Points**:
- Event ingestion rate
- Processing latency per pipeline stage
- Adapter-specific performance
- Filter effectiveness
- Error rates and types
- Pool discovery rate
- REST API call latency
- Pool state cache hit/miss ratios

**Prometheus Integration**:

```rust
pub struct IngestorMetrics {
    events_processed: Counter,
    processing_latency: Histogram,
    adapter_errors: CounterVec,
    filter_drops: Counter,
    pools_discovered: Counter,
    pool_state_fetch_latency: Histogram,
    pool_cache_hits: Counter,
    pool_cache_misses: Counter,
}
```

## Data Models

### Core Event Types

```rust
#[derive(Debug, Clone)]
pub struct RawEvent {
    pub timestamp: DateTime<Utc>,
    pub block_height: u64,
    pub transaction_hash: String,
    pub event_data: Vec<u8>,
    pub module_address: AccountAddress,
}

#[derive(Debug, Clone)]
pub struct TimestampedEvent {
    pub event: RawEvent,
    pub processing_timestamp: DateTime<Utc>,
    pub sequence_number: u64,
}
```

### Pool State Models

```rust
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct PoolId {
    pub dex: String,
    pub address: String,
    pub token_pair: (String, String),
}

#[derive(Debug, Clone)]
pub struct PoolState {
    pub pool_id: PoolId,
    pub reserves: (Decimal, Decimal),
    pub fee_rate: Decimal,
    pub last_updated: DateTime<Utc>,
    pub block_height: u64,
    pub additional_data: serde_json::Value, // DEX-specific data
}

#[derive(Debug)]
pub enum DetectorMessage {
    BlockStart {
        block_number: u64,
        timestamp: DateTime<Utc>,
    },
    MarketUpdate(MarketUpdate),
    PoolInitialization(PoolState),
    BlockEnd {
        block_number: u64,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum PoolStateError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    
    #[error("Pool not found: {0:?}")]
    PoolNotFound(PoolId),
    
    #[error("Invalid pool data: {0}")]
    InvalidData(String),
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
}
```

### Configuration Models

```rust
#[derive(Deserialize)]
pub struct IngestorConfig {
    pub data_source: DataSourceConfig,
    pub filters: FilterConfig,
    pub performance: PerformanceConfig,
    pub adapters: Vec<AdapterConfig>,
    pub pool_state: PoolStateConfig,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum DataSourceConfig {
    Grpc { endpoint: String, timeout_ms: u64 },
    File { path: String, replay_speed: Option<f64> },
}
```

## Error Handling

### Error Hierarchy

```rust
#[derive(thiserror::Error, Debug)]
pub enum IngestorError {
    #[error("Data source error: {0}")]
    DataSource(#[from] DataSourceError),
    
    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),
    
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),
    
    #[error("Channel error: {0}")]
    Channel(String),
}
```

### Recovery Strategies

**Connection Failures**: Exponential backoff with jitter, maximum retry attempts
**Parsing Failures**: Log error, skip event, continue processing
**Channel Backpressure**: Apply flow control, emit warnings
**Adapter Failures**: Isolate failing adapter, continue with others

**Design Decision**: Fail-fast for configuration errors, resilient recovery for runtime errors.

**Rationale**: Configuration errors indicate fundamental setup issues, while runtime errors are often transient.

## Testing Strategy

### Unit Testing

- **Adapter Testing**: Mock event data for each DEX adapter
- **Filter Testing**: Validate filtering logic with synthetic market updates
- **Configuration Testing**: Test all valid configuration combinations
- **Error Handling**: Simulate failure conditions and verify recovery

### Integration Testing

- **End-to-End Pipeline**: Use recorded data files for full pipeline testing
- **Data Source Switching**: Verify identical behavior between gRPC and file sources
- **Performance Testing**: Benchmark critical paths with realistic data volumes
- **Stress Testing**: High-volume event processing with resource monitoring

### Test Data Management

**Recorded Data Sets**:
- Mainnet samples for realistic testing
- Synthetic edge cases for error condition testing
- Performance benchmark datasets

**Test Utilities**:
- Mock data generators for unit tests
- Configuration builders for test scenarios
- Assertion helpers for market update validation

### Performance Benchmarks

**Target Metrics**:
- Event processing: < 1ms per event
- End-to-end latency: < 10ms (ingestion to detector delivery)
- Memory usage: < 100MB steady state
- CPU usage: < 20% on single core

**Benchmark Coverage**:
- Individual adapter performance
- Filter processing overhead
- Channel throughput limits
- Memory allocation patterns

## Implementation Considerations

### Async Architecture

**Design Decision**: Maintain existing tokio-based async architecture with channel communication.

**Rationale**: Preserves compatibility with existing detector component and provides necessary concurrency for multiple data sources.

### Memory Management

**Strategy**: Pre-allocate buffers for high-frequency operations, use object pools for event structures.

**Rationale**: Reduces GC pressure and allocation overhead in hot paths.

### Configuration Hot-Reloading

**Future Enhancement**: Support runtime configuration updates without restart.

**Current Approach**: Require restart for configuration changes to maintain simplicity.

### Observability

**Logging Strategy**: Structured logging with correlation IDs for request tracing.

**Metrics Strategy**: Prometheus metrics with Grafana dashboards for operational monitoring.

**Tracing Strategy**: Optional distributed tracing for performance debugging.