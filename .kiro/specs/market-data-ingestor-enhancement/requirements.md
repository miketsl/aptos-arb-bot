# Requirements Document

## Introduction

The Market Data Ingestor (MDI) is a critical component of the Aptos arbitrage bot that monitors multiple DEXes, processes blockchain events, and feeds market updates to the detector. The current implementation has successfully delivered core functionality including configurable data sources (gRPC and file replay), DEX adapter system, and basic filtering. However, it needs additional enhancements to support production-grade requirements including pool state management for cold boot scenarios, comprehensive test tooling, robust error handling, and performance monitoring.

## Requirements

### Requirement 1 

**User Story:** As a developer, I want configurable data sources so that I can seamlessly switch between live gRPC streams and recorded test data for development and testing.

#### Acceptance Criteria

1. WHEN the system starts THEN it SHALL load data source configuration from YAML and initialize the appropriate source type
2. WHEN configured for gRPC source THEN the system SHALL connect to live Aptos blockchain data stream
3. WHEN configured for file source THEN the system SHALL replay recorded protobuf data at configurable speed
4. IF file replay speed is set to 0 THEN the system SHALL process data as fast as possible without timing delays
5. WHEN switching between data sources THEN the system SHALL maintain identical processing behavior and output format

### Requirement 2

**User Story:** As a developer, I want bidirectional test data recording and editing tools so that I can capture live data with embedded pool state, manually edit test scenarios, and replay modified data for comprehensive testing.

#### Acceptance Criteria

1. WHEN recording tool is executed THEN it SHALL capture raw transaction data from gRPC stream in protobuf format
2. WHEN recording data THEN the system SHALL preserve exact timing metadata and transaction ordering
3. WHEN recording encounters unknown pools THEN it SHALL fetch pool state via REST API and embed it in the recorded batch
4. WHEN converting protobuf to JSON THEN the system SHALL export human-readable format suitable for manual editing including embedded pool states
5. WHEN a human edits JSON data THEN they SHALL be able to reorder events, modify values, add/remove transactions, and edit pool state data
6. WHEN converting JSON back to protobuf THEN the system SHALL validate data integrity and recreate binary format for replay including pool state data
7. WHEN replaying converted data THEN the system SHALL use embedded pool states instead of making REST API calls
8. WHEN recording or conversion fails THEN the system SHALL log errors and continue processing without data loss

### Requirement 3 

**User Story:** As a system operator, I want configurable pool filtering so that I can focus processing on specific tokens or token pairs and reduce computational overhead.

#### Acceptance Criteria

1. WHEN token filters are configured THEN the system SHALL process only pools containing those tokens
2. WHEN token pair filters are configured THEN the system SHALL process only pools matching those specific combinations
3. WHEN no token filters are configured THEN the system SHALL process all pools discovered from configured DEX modules
4. WHEN applying filters THEN the system SHALL drop unwanted market updates after parsing but before sending to detector
5. WHEN DEX adapters parse events THEN they SHALL use type-safe, code-based parsing for performance and reliability
6. WHEN new event types are needed THEN they SHALL be added through adapter code changes rather than configuration mapping

### Requirement 4

**User Story:** As a developer, I want a generic DEX adapter system so that I can easily add support for new DEX protocols without modifying core processing logic.

#### Acceptance Criteria

1. WHEN a new DEX adapter is implemented THEN it SHALL conform to the standard DexAdapter trait interface
2. WHEN processing events THEN the system SHALL automatically route events to appropriate adapters based on module address
3. WHEN an adapter parses an event THEN it SHALL return standardized MarketUpdate structures
4. WHEN multiple adapters are registered THEN the system SHALL route events to appropriate adapters and process all DEX types in the same pipeline
5. WHEN an adapter fails THEN the system SHALL log the error and continue processing other adapters

### Requirement 5

**User Story:** As a system operator, I want comprehensive performance monitoring so that I can ensure the system meets sub-100ms latency requirements.

#### Acceptance Criteria

1. WHEN processing events THEN the system SHALL measure and log processing time for each pipeline stage
2. WHEN latency exceeds thresholds THEN the system SHALL emit performance warnings
3. WHEN system is running THEN it SHALL expose Prometheus metrics for monitoring
4. WHEN processing batches THEN the system SHALL track throughput and queue depths
5. WHEN performance degrades THEN the system SHALL provide detailed timing breakdowns for debugging

### Requirement 6

**User Story:** As a developer, I want robust error handling and recovery so that temporary failures don't crash the entire system.

#### Acceptance Criteria

1. WHEN gRPC connection fails THEN the system SHALL attempt reconnection with exponential backoff
2. WHEN event parsing fails THEN the system SHALL log the error and continue processing other events
3. WHEN adapter throws exception THEN the system SHALL isolate the failure and continue with other adapters
4. WHEN downstream channel is full THEN the system SHALL apply backpressure and log warnings
5. WHEN unrecoverable error occurs THEN the system SHALL shutdown gracefully and report the cause

### Requirement 7

**User Story:** As a system operator, I want automatic pool discovery and state initialization so that the detector graph always has complete pool information when processing market updates.

#### Acceptance Criteria

1. WHEN processing an event containing an unknown pool THEN the system SHALL detect that the pool is not in the known pools registry
2. WHEN an unknown pool is detected THEN the system SHALL fetch complete pool state via REST API from the appropriate DEX
3. WHEN pool state is fetched THEN the system SHALL send PoolInitialization message to detector before processing any market updates for that pool
4. WHEN pool state fetch fails THEN the system SHALL retry with exponential backoff and log errors
5. WHEN pool state is successfully fetched THEN the system SHALL add the pool to the known pools registry
6. WHEN pool state is cached THEN the system SHALL respect TTL and refresh stale entries
7. WHEN multiple events reference the same unknown pool THEN the system SHALL fetch state only once and queue subsequent events

### Requirement 8

**User Story:** As a developer, I want file replay to use historically consistent pool state so that replayed transactions match the original market conditions without fetching current/live pool state.

#### Acceptance Criteria

1. WHEN replaying recorded data THEN the system SHALL use embedded historical pool state data instead of making REST API calls
2. WHEN recording live data THEN the system SHALL detect unknown pools and embed their historical state in the recorded batch
3. WHEN file replay encounters an unknown pool THEN it SHALL initialize the pool using embedded state data from the replay file
4. WHEN embedded pool state is missing for a required pool THEN the system SHALL log an error and skip processing that pool's events
5. WHEN converting between protobuf and JSON formats THEN the system SHALL preserve embedded pool state data
6. WHEN replaying data THEN pool state timestamps SHALL match the historical context of the recorded transactions
7. WHEN file replay is active THEN the system SHALL never make REST API calls for pool state initialization

### Requirement 9

**User Story:** As a developer, I want comprehensive testing capabilities so that I can validate system behavior under various conditions.

#### Acceptance Criteria

1. WHEN running unit tests THEN each adapter SHALL be testable in isolation with mock data
2. WHEN running integration tests THEN the system SHALL support end-to-end testing with recorded data
3. WHEN testing performance THEN the system SHALL include benchmarks for critical processing paths
4. WHEN testing error conditions THEN the system SHALL validate proper error handling and recovery
5. WHEN testing configuration THEN the system SHALL validate all configuration combinations work correctly
6. WHEN testing pool discovery THEN the system SHALL validate pool state fetching and caching behavior