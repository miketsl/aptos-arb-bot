# Implementation Plan

- [x] 1. Set up enhanced configuration system and data models
  - Create new configuration structures for data sources, filters, and performance settings
  - Implement YAML deserialization with validation for all configuration options
  - Add configuration loading utilities with environment-specific overrides
  - Write unit tests for configuration parsing and validation
  - _Requirements: 1.1, 3.1, 3.3_

- [x] 2. Implement core data source abstraction and trait system
  - Define DataSource trait with async methods for start, next_event, and stop
  - Create RawEvent and TimestampedEvent data structures
  - Implement error types for data source operations
  - Write unit tests for data structures and trait interface
  - _Requirements: 1.1, 1.5_

- [x] 3. Build gRPC data source implementation
  - Implement GrpcDataSource struct with connection management
  - Add exponential backoff reconnection strategy with configurable parameters
  - Implement connection health monitoring and graceful degradation
  - Create unit tests with mock gRPC client for connection scenarios
  - Write integration tests with actual gRPC endpoint
  - _Requirements: 1.2, 6.1_

- [x] 4. Create enhanced file-based data source for replay functionality with embedded pool state
  - Implement enhanced RecordedBatch structure with pool_initializations field
  - Update FileDataSource to handle embedded pool state data during replay
  - Add configurable replay speed with timing preservation
  - Implement fast-forward mode when replay speed is 0
  - Create unit tests with sample protobuf files including embedded pool states
  - Write integration tests for timing accuracy, event ordering, and pool state initialization
  - _Requirements: 1.3, 1.4, 1.5, 8.1, 8.3_

- [ ] 5. Develop enhanced test data recording and conversion tooling with pool state embedding
  - Create recording utility that captures gRPC stream to protobuf files with pool state detection
  - Implement pool state fetching during recording when unknown pools are discovered
  - Add pool state embedding into RecordedBatch structures during live recording
  - Implement bidirectional protobuf to JSON converter with timing metadata and pool state preservation
  - Add data integrity validation for conversion cycles including pool state data
  - Create command-line tools for recording and conversion operations with pool state support
  - Write unit tests for conversion accuracy, data integrity, and pool state embedding
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 8.2_

- [x] 6. Implement enhanced DEX adapter system
  - Refactor existing DexAdapter trait to include module_addresses method
  - Create EventRouter for module address-based event routing
  - Update existing DEX adapters (Hyperion, Thala, Tapp) to new interface
  - Implement adapter registration and routing logic
  - Write unit tests for event routing and adapter isolation
  - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

- [ ] 7. Build configurable filtering system
  - Create FilterConfig structure with token and DEX filtering options
  - Implement MarketUpdateFilter with multi-level filtering logic
  - Add filter application after parsing but before detector delivery
  - Create filter effectiveness metrics and logging
  - Write unit tests for all filter combinations and edge cases
  - _Requirements: 3.1, 3.2, 3.3, 3.4_

- [ ] 8. Add comprehensive performance monitoring
  - Implement IngestorMetrics with Prometheus counter and histogram metrics
  - Add timing measurements for each pipeline stage
  - Create performance warning system for latency threshold violations
  - Implement throughput and queue depth tracking
  - Write unit tests for metrics collection and threshold detection
  - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

- [ ] 9. Implement robust error handling and recovery
  - Create comprehensive error hierarchy with structured error types
  - Implement exponential backoff for connection failures
  - Add error isolation for adapter failures with continued processing
  - Implement backpressure handling for channel congestion
  - Write unit tests for all error scenarios and recovery paths
  - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

- [ ] 10. Create comprehensive test suite and benchmarks
  - Implement unit tests for all components with mock data
  - Create integration tests using recorded data files
  - Build performance benchmarks for critical processing paths
  - Add stress testing with high-volume event processing
  - Create test utilities for mock data generation and validation
  - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5_

- [ ] 11. Integrate enhanced MDI with existing detector component
  - Update main application to use new configuration system
  - Modify detector integration to handle new data source abstraction
  - Ensure backward compatibility with existing detector interface
  - Add end-to-end integration tests with detector component
  - Write performance tests for complete pipeline latency
  - _Requirements: 1.5, 5.1_

- [ ] 12. Implement data source-aware pool state management system for cold boot problem
  - Create PoolStateManager struct with data source type awareness and multiple registries
  - Implement thread-safe registries for known_pools, rejected_pools, pending_pools, and new_pool_state_cache
  - Add data source-specific pool discovery logic (REST API for live, embedded data for replay)
  - Create pool filtering logic to determine which unknown pools are worth tracking
  - Implement file replay pool state initialization from embedded RecordedPoolState data
  - Write unit tests for both live and replay pool discovery scenarios
  - _Requirements: 7.1, 7.5, 7.6, 8.1, 8.3_

- [ ] 13. Build separate DEX-specific pool state providers for REST API integration
  - Create new PoolStateProvider implementations as separate components from DEX adapters
  - Implement HyperionStateProvider with async HTTP client for Hyperion REST API endpoints
  - Create ThalaStateProvider for Thala DEX REST API with rate limiting and proper error handling
  - Add TappStateProvider for Tapp protocol REST API with retry logic and timeout handling
  - Implement concurrent pool state fetching for multiple pools from same DEX provider
  - Write unit tests with mock HTTP clients and async test utilities for each provider
  - _Requirements: 7.2, 7.4_

- [ ] 14. Integrate data source-aware pool discovery with block-aligned event processing
  - Modify DEX adapters to extract pool references and check registries before processing
  - Implement event dropping logic for newly discovered pools during live async state fetch
  - Add embedded pool state processing at batch start for file replay scenarios
  - Add new_pool_state_cache checking at BlockStart to send PoolInitialization messages for live streams
  - Ensure proper ordering of PoolInitialization before MarketUpdate messages within blocks
  - Write integration tests for both live and replay pool discovery flows with block boundaries
  - _Requirements: 7.1, 7.3, 7.7, 8.1, 8.3, 8.7_

- [ ] 15. Add observability and operational tooling
  - Implement structured logging with correlation IDs
  - Create Grafana dashboard configurations for monitoring
  - Add health check endpoints for operational monitoring
  - Implement graceful shutdown handling for all components
  - Write documentation for operational procedures and troubleshooting
  - _Requirements: 5.3, 6.5_