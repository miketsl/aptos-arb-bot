# Task 16: MDI Crate Refactoring - Clean Architecture & Unified Configuration

## Overview
Transform the messy MDI crate into a clean, maintainable, enterprise-grade codebase by eliminating configuration duplication, reorganizing modules, and removing backward compatibility bloat.

## Current Problems Summary
- **Triple Config System:** 3 overlapping config structures with duplication
- **Confused Data Sources:** Aptos indexer and MDI data source are the same thing but configured separately
- **Flat Module Structure:** 11 modules in root with no logical organization
- **Mixed Concerns:** CLI tools, domain logic, and configuration scattered
- **44 Public Exports:** Unclear API surface with too many exposed internals

## Phase 1: Configuration System Overhaul (Week 1)

### 1.1 Delete Legacy Configuration (Days 1-2)
**Remove from `crates/config/src/lib.rs`:**
- `MarketDataConfig` struct and enum variants
- `FilterConfig` enum (TokenPairs, Token, All variants)
- `DexConfig` struct (duplicate of AdapterConfig)
- `YamlTransactionStreamConfig` struct
- All backward compatibility logic in `apply_environment_overrides()`
- Legacy validation functions for old config types

**Remove from config files:**
- `market_data_config` section from `config/default.yml`
- `detector_config` section (move to detector crate)
- `transaction_stream_config` section (merge into data_source)

### 1.2 Design Unified Configuration (Days 3-4)
**Create new config types in `crates/config/src/lib.rs`:**
- `MdiConfig` - root configuration struct
- `DataSourceConfig` - unified gRPC + file sources (replaces indexer config)
- `ProcessingConfig` - batch sizes, timeouts, buffers
- `PoolStateConfig` - cache settings, worker pools
- `FilterConfig` - token/pair filtering (simplified)
- `DexConfig` - DEX adapter configuration (clean up existing)
- `MonitoringConfig` - observability settings
- `RecordingConfig` - optional, only for recording tools

**Key Design Principles:**
- No duplicate settings between structs
- DataSourceConfig handles both Aptos indexer SDK and MDI needs
- Environment variable support with `${VAR}` syntax
- Comprehensive validation with helpful error messages
- Optional sections for features not always used

### 1.3 Update Configuration Files (Day 5)
**Rewrite `config/default.yml`:**
- Single `mdi` root section
- Unified `data_source` (grpc OR file, not both)
- Logical grouping of related settings
- Environment variable examples
- Remove all legacy sections

**Create additional config files:**
- `config/development.yml` - file replay configuration
- `config/production.yml` - production gRPC configuration
- `config/test.yml` - test configuration with minimal settings

## Phase 2: Module Architecture Restructuring (Week 2)

### 2.1 Create New Module Hierarchy (Days 1-2)
**New structure in `crates/market-data-ingestor/src/`:**
```
config/           # Configuration management
├── mod.rs
├── mdi.rs       # MdiConfig and related types  
├── validation.rs # Config validation logic
└── environment.rs # Environment variable handling

core/            # Core business logic
├── mod.rs
├── processor.rs # Main processor (cleaned up)
└── pipeline/    # Processing pipeline
    ├── mod.rs
    ├── extractor.rs  # Event extraction (from steps/)
    ├── parser.rs     # Event parsing (from steps/)
    ├── filter.rs     # Filtering logic (from steps/)
    └── detector.rs   # Detector push (from steps/)

sources/         # Data source abstractions  
├── mod.rs
├── grpc.rs      # gRPC source (cleaned up)
├── file.rs      # File source (cleaned up)
└── traits.rs    # DataSource trait and common types

pool_state/      # Pool state management
├── mod.rs
├── manager.rs   # PoolStateManager (from pool_state_manager.rs)
├── cache.rs     # Caching logic
└── discovery.rs # Pool discovery logic

monitoring/      # Observability & monitoring
├── mod.rs  
├── metrics.rs   # Prometheus metrics (from monitoring.rs)
├── health.rs    # Health check endpoints
└── server.rs    # HTTP server (from http_server.rs)

recording/       # Recording functionality
├── mod.rs
├── recorder.rs  # Recording logic
├── rotation.rs  # File rotation (from file_rotation.rs)
└── monitor.rs   # Recording monitor (from recording_monitor.rs)

types/           # Domain types only
├── mod.rs
├── events.rs    # Event types from types.rs
├── pools.rs     # Pool-related types
└── market.rs    # Market data types

tools/           # CLI tools (separated from lib)
├── mod.rs
├── recorder.rs  # mdi-recorder functionality
├── inspector.rs # mdi-inspector functionality
├── converter.rs # mdi-converter functionality
└── shared.rs    # Shared CLI utilities
```

### 2.2 Move and Reorganize Existing Code (Days 3-4)
**File movements and transformations:**
- `ingestor_config.rs` → `config/mdi.rs` (cleaned up)
- `steps/` → `core/pipeline/` (restructured)
- `data_source/` → `sources/` (cleaned up)
- `pool_state_manager.rs` → `pool_state/manager.rs` (split up)
- `monitoring.rs` → `monitoring/metrics.rs` (split up)
- `http_server.rs` → `monitoring/server.rs` (cleaned up)
- `recording_*.rs` → `recording/` (organized)
- `types.rs` → `types/` (split by domain)
- `bin/` → `tools/` (reorganized)

**Update all internal imports and module declarations**

### 2.3 Clean Up Public API (Day 5)
**Drastically reduce exports in `lib.rs`:**
- Export only essential types needed by other crates
- Use re-exports strategically from submodules
- Document public API with comprehensive examples
- Hide implementation details

**Target: Reduce from 44 exports to ~10-15 essential exports**

## Phase 3: Type System Consolidation (Week 3)

### 3.1 Eliminate Type Duplication (Days 1-2)
**Merge duplicate configuration types:**
- Consolidate `DexConfig` (types.rs) and `AdapterConfig` (config.rs)
- Remove duplicate event type definitions
- Unify pool state representations
- Create single source of truth for each domain concept

### 3.2 Create Consistent Domain Model (Days 3-4)
**Organize types by domain in `types/`:**
- `events.rs` - All event-related types (PoolSnapshot, SwapAfter, etc.)
- `pools.rs` - Pool state, cache, discovery types
- `market.rs` - Market updates, token pairs, tick info

**Establish naming conventions:**
- Config types: `*Config` (e.g., `DexConfig`, `FilterConfig`)
- State types: `*State` (e.g., `PoolState`, `ConnectionState`)
- Event types: `*Event` (e.g., `SwapEvent`, `SnapshotEvent`)
- Error types: `*Error` (e.g., `ConfigError`, `SourceError`)

### 3.3 Implement Comprehensive Error Handling (Day 5)
**Create module-specific error types:**
- `config::ConfigError` - configuration validation errors
- `sources::SourceError` - data source connection/parsing errors
- `pool_state::PoolError` - pool state management errors
- `recording::RecordingError` - recording operation errors

**Implement proper error context and propagation throughout**

## Phase 4: Core Logic Refactoring (Week 4)

### 4.1 Pipeline Architecture Implementation (Days 1-2)
**Transform `steps/` into clean `core/pipeline/`:**
- Define clear `Step` trait for pipeline components
- Implement error handling and recovery in pipeline
- Add metrics and observability to each step
- Create composable pipeline builder

### 4.2 Pool State Management Overhaul (Days 3-4)
**Restructure pool state logic:**
- Separate concerns: caching, discovery, state management
- Optimize cache performance and memory usage
- Implement proper concurrent access patterns
- Add comprehensive metrics for pool operations

### 4.3 Configuration Integration (Day 5)
**Wire new configuration system throughout:**
- Update all components to use new `MdiConfig`
- Remove references to legacy config types
- Implement configuration hot-reloading (if needed)
- Add configuration validation at startup

## Phase 5: CLI Tools Separation (Week 5)

### 5.1 Extract CLI Tools to Separate Module (Days 1-3)
**Move CLI binaries to `tools/` module:**
- Create clean separation between library and CLI code
- Implement shared utilities for consistent CLI patterns
- Add proper error handling and user-friendly messages
- Create unified CLI help and documentation

### 5.2 Recording Tools Integration (Days 4-5)
**Integrate recording with new architecture:**
- Use new configuration system for recording tools
- Implement proper lifecycle management
- Add progress reporting and monitoring
- Create reusable recording components

## Phase 6: Testing, Documentation & Validation (Week 6)

### 6.1 Test Suite Overhaul (Days 1-2)
**Fix and enhance tests:**
- Update all tests for new module structure
- Add comprehensive tests for new configuration system
- Create integration tests for new architecture
- Add performance benchmarks for critical paths

### 6.2 Documentation and Migration (Days 3-4)
**Create comprehensive documentation:**
- Update README with new configuration format
- Document new module architecture and design decisions
- Create configuration migration examples
- Add troubleshooting guide for common issues

### 6.3 Final Validation and Polish (Days 5-6)
**Ensure production readiness:**
- Run comprehensive test suite
- Validate configuration examples work correctly
- Check performance benchmarks meet requirements
- Review code for consistency and best practices
- Create deployment examples and guides

## New Unified Configuration Format

### Clean Configuration Structure
```yaml
# Clean, unified configuration
mdi:
  # Single data source configuration (replaces both indexer + mdi source)
  data_source:
    # For live gRPC streaming
    type: grpc
    endpoint: "https://grpc.mainnet.aptoslabs.com:443"
    auth_token: "${APTOS_AUTH_TOKEN}"
    starting_version: null  # null = latest, or specific block number
    request_header: "market-data-ingestor"
    timeout_ms: 30000
    reconnect:
      max_attempts: 5
      backoff_base_ms: 1000
    
    # OR for file replay (alternative to grpc)
    # type: file
    # path: "./recordings/mainnet.pb"
    # replay_speed: 1.0  # 1.0 = real-time, null = as fast as possible

  # Processing configuration
  processing:
    batch_size: 1000
    batch_timeout_ms: 100
    buffer_size: 10000
    latency_threshold_ms: 1000

  # Pool state management
  pool_state:
    cache_ttl_seconds: 300
    max_cache_size: 10000
    fetch_timeout_ms: 5000
    max_workers: 10
    retry_attempts: 3

  # Market data filtering
  filters:
    enabled: true
    tokens: ["APT", "USDC", "USDT"]
    token_pairs:
      - ["APT", "USDC"] 
      - ["APT", "USDT"]
    min_liquidity: "1000"
    dex_whitelist: ["hyperion", "thala"]

  # DEX adapters (what DEXs to monitor)
  dexs:
    hyperion:
      module_address: "0x89576037b3cc0b89645ea393a47787bb348272c76d6941c574b053672b848039"
      enabled: true
      events:
        swap: "::pool::SwapAfterEvent" 
        snapshot: "::pool::PoolSnapshot"
      settings:
        tick_spacing_threshold: 10
    
    thala:
      module_address: "0x48271d39d0b05bd6efca2278f22277d6fcc375504f9839fd73f74ace240861af"
      enabled: true
      events:
        swap: "::pool::SwapAfterEvent"
        snapshot: "::pool::PoolSnapshot"
      settings: {}

  # Monitoring & observability
  monitoring:
    enabled: true
    port: 8080
    dashboard: true
    prometheus: true
    health_checks: true

# Optional: Recording configuration (only when using recording tools)
recording:
  output_pattern: "recordings/mdi_{timestamp}.pb"
  rotation:
    enabled: true
    max_size_mb: 100
    max_files: 10
    compress: true
  monitoring:
    progress_interval_seconds: 30
    pool_detection: true
```

### Key Configuration Improvements
1. **Single Data Source:** No more confusion between "indexer config" and "MDI data source" - they're the same thing
2. **Clear Alternatives:** Either `grpc` OR `file` - not both at once
3. **Environment Variables:** Built-in support with `${VAR}` syntax
4. **Logical Grouping:** Related settings grouped together (reconnect settings under data_source)
5. **No Duplication:** Each setting appears exactly once
6. **Optional Sections:** Recording config only exists when using recording tools

## Success Criteria

### Configuration Quality
- ✅ Single source of truth for all configuration
- ✅ No duplicate or conflicting settings
- ✅ Clear data source configuration (gRPC OR file)
- ✅ Environment variable support throughout
- ✅ Comprehensive validation with helpful errors

### Code Organization  
- ✅ Logical module hierarchy with clear responsibilities
- ✅ Separation of concerns (CLI vs library vs tools)
- ✅ Clean public API with minimal exports
- ✅ Consistent naming and coding patterns

### Enterprise Readiness
- ✅ Comprehensive error handling with context
- ✅ Full test coverage for new architecture
- ✅ Performance meets or exceeds current benchmarks
- ✅ Documentation covers all use cases
- ✅ Ready for production deployment

This refactoring will transform the MDI crate from a messy collection of components into a clean, professional, enterprise-grade codebase that's maintainable and extensible.