# Detector Service Test Suite

This directory contains comprehensive tests for the detector service, organized into three categories:

## Test Categories

### 1. Unit Tests (`src/`)
Located within the source code modules, these test individual components:
- **Graph operations**: Edge updates, pruning, view creation
- **Strategy logic**: Individual strategy detection algorithms  
- **Transform layer**: MarketUpdate to Edge conversion
- **Deduplication**: Opportunity deduplication logic
- **Service components**: Error handling, metrics

### 2. Integration Tests (`tests/`)

#### `integration.rs` & `cross_dex_integration.rs`
Basic integration tests verifying end-to-end functionality:
- Triangular arbitrage detection with CLMM pools
- Cross-DEX arbitrage detection with service orchestration

#### `comprehensive_integration.rs`
Advanced integration scenarios:
- **Multi-strategy detection**: All strategies running simultaneously
- **High-frequency updates**: Rapid market data processing
- **CLMM integration**: Concentrated liquidity pool handling
- **Error handling**: Graceful degradation with malformed data
- **Graph pruning**: Large graph management

#### `performance_benchmarks.rs`
Performance and latency measurements:
- **Single strategy latency**: < 10ms average detection time
- **Multi-strategy performance**: Parallel execution efficiency
- **Update throughput**: > 1000 updates/second processing
- **Memory usage**: Large graph handling
- **Strategy-specific benchmarks**: Individual strategy performance

#### `stress_tests.rs`
Extreme condition testing:
- **Massive updates**: 10,000+ updates across 100 blocks
- **Rapid blocks**: 1000 blocks with minimal latency
- **Extreme values**: Edge cases with unusual price/liquidity values
- **Channel saturation**: Backpressure handling
- **Large tick maps**: Memory pressure with CLMM data
- **Disconnected components**: Graph topology edge cases
- **Strategy load**: Maximum strategy configuration

## Running Tests

```bash
# Run all tests
cargo test

# Run specific test categories
cargo test --lib                    # Unit tests only
cargo test --tests                  # Integration tests only

# Run specific test files
cargo test --test comprehensive_integration
cargo test --test performance_benchmarks
cargo test --test stress_tests

# Run specific tests
cargo test test_multi_strategy_detection
cargo test benchmark_single_strategy_latency
cargo test stress_test_massive_updates

# Run with output
cargo test -- --nocapture
```

## Performance Targets

Based on architecture requirements:

| Metric | Target | Test |
|--------|--------|------|
| Single strategy latency | < 10ms avg | `benchmark_single_strategy_latency` |
| Multi-strategy latency | < 200ms avg | `benchmark_multi_strategy_performance` |
| Update throughput | > 1000/sec | `benchmark_update_throughput` |
| Memory usage | < 1GB for 10K edges | `benchmark_memory_usage` |
| Error recovery | Graceful degradation | `test_error_handling` |

## Test Data Patterns

### Market Scenarios
- **Cross-DEX arbitrage**: Same pair, different prices across DEXes
- **Triangular arbitrage**: A→B→C→A profitable cycles
- **Multi-hop arbitrage**: Longer paths across multiple DEXes
- **CLMM pools**: Concentrated liquidity with tick data

### Stress Patterns
- **High frequency**: Rapid price updates
- **Large graphs**: Many tokens and trading pairs
- **Extreme values**: Edge cases for price/liquidity
- **Disconnected components**: Isolated trading clusters

## Adding New Tests

When adding new tests:

1. **Unit tests**: Add to relevant `src/` module
2. **Integration tests**: Add to appropriate `tests/` file
3. **Performance tests**: Add to `performance_benchmarks.rs`
4. **Stress tests**: Add to `stress_tests.rs`

### Test Naming Convention
- Unit tests: `test_<functionality>`
- Integration tests: `test_<scenario>_integration`
- Benchmarks: `benchmark_<metric>`
- Stress tests: `stress_test_<condition>`

### Helper Functions
Use the provided helper functions for consistency:
- `create_market_update()`: Standard market updates
- `create_clmm_update()`: CLMM pools with tick data
- Standard timeout patterns for async operations

## Continuous Integration

All tests should pass in CI environments. Performance tests include reasonable tolerances for different hardware configurations.

The test suite provides confidence in:
- ✅ Correctness of arbitrage detection
- ✅ Performance under load
- ✅ Reliability with edge cases
- ✅ Graceful error handling
- ✅ Memory efficiency
- ✅ Concurrent operation safety