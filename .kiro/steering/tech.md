# Technology Stack

## Build System & Toolchain
- **Language**: Rust (stable channel)
- **Build System**: Cargo workspace with multiple crates
- **Toolchain**: Standard Rust toolchain (rust-toolchain.toml specifies stable)

## Core Dependencies
- **Async Runtime**: `tokio` with full features for async/await patterns
- **Graph Processing**: `petgraph` for price graph data structures
- **Decimal Math**: `rust_decimal` with math features for precise financial calculations
- **Serialization**: `serde` with derive features for JSON/YAML config
- **Error Handling**: `thiserror` and `anyhow` for structured error management
- **Logging**: `log` crate for structured logging
- **Metrics**: `prometheus` for performance monitoring
- **Time**: `chrono` with serde support for timestamps

## Aptos Integration
- **SDK**: Direct git dependency on `aptos-core` devnet branch
- **Indexer**: `aptos-indexer-processor-sdk` for blockchain data ingestion
- **Testing**: Optional `aptos-tests` feature for integration testing utilities

## Common Commands

### Development
```bash
# Build entire workspace
cargo build

# Run tests
cargo test

# Run specific crate tests
cargo test -p detector

# Check code formatting
cargo fmt --check

# Run clippy lints
cargo clippy --deny warnings

# Run with specific features
cargo test --features aptos-tests
```

### Performance & Debugging
```bash
# Build optimized release
cargo build --release

# Run benchmarks (when available)
cargo bench

# Profile with flamegraph
cargo flamegraph --bin arb-bot

# Check dependencies
cargo tree
```

## Architecture Patterns
- **Workspace Structure**: Multi-crate workspace for modular components
- **Async-First**: All I/O operations use tokio async patterns
- **Channel Communication**: `tokio::sync::mpsc` and `broadcast` for inter-component messaging
- **Trait-Based Design**: Common interfaces like `DexAdapter` for extensibility
- **Configuration-Driven**: YAML configuration files for runtime behavior
- **Error Propagation**: Consistent use of `Result<T, E>` with structured errors

## Performance Considerations
- **Zero-Copy Parsing**: Minimize allocations in hot paths
- **SIMD Math**: Planned use of `packed_simd_2` for price calculations
- **Memory Pools**: Pre-allocated structures for graph operations
- **Parallel Processing**: Strategy engines run concurrently
- **Block-Aligned Processing**: Batch updates within Aptos block boundaries (~130ms)