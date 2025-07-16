# Project Structure

## Workspace Organization

This is a Cargo workspace with multiple crates organized by functional domain:

```
aptos-arb-bot/
├── crates/                    # Core library crates
│   ├── common/               # Shared types, errors, traits, utilities
│   ├── config/               # Configuration management
│   ├── core/                 # Orchestration, DI container, risk manager
│   ├── dex-adapter-trait/    # Common DEX interface definition
│   ├── dex-adapters/         # Concrete DEX implementations (Hyperion, Thala, Tapp)
│   ├── detector/             # Arbitrage opportunity detection engine
│   ├── market-data-ingestor/ # Blockchain data ingestion and processing
│   ├── executor/             # Trade execution (stub)
│   └── analytics/            # Async telemetry and metrics (stub)
├── bin/                      # Binary executables
├── config/                   # Runtime configuration files (YAML)
├── docs/                     # Architecture and design documentation
└── recordings/               # Sample data for testing/replay
```

## Crate Responsibilities

### Core Libraries
- **common**: Foundation types (`TokenPair`, `Edge`, `ArbitrageOpportunity`), error types, shared traits
- **config**: Configuration loading and validation from YAML files
- **core**: High-level orchestration, dependency injection, risk management logic

### Data Pipeline
- **market-data-ingestor**: Ingests Aptos blockchain events, transforms to market updates
- **detector**: Price graph maintenance, arbitrage detection algorithms (Bellman-Ford)
- **executor**: Transaction building, signing, and submission (currently stub)

### DEX Integration
- **dex-adapter-trait**: Common interface (`DexAdapter` trait) for all DEX integrations
- **dex-adapters**: Concrete implementations for Hyperion, ThalaSwap, Tapp protocols

### Supporting
- **analytics**: Async telemetry, Prometheus metrics, data persistence (stub)

## Key Architectural Patterns

### Modular Design
- Each crate has a single, well-defined responsibility
- Dependencies flow in one direction (no circular dependencies)
- Common functionality extracted to `common` crate

### Configuration-Driven
- Runtime behavior controlled via `config/default.yml`
- DEX configurations, strategy parameters, and system settings externalized
- Environment-specific overrides supported

### Async Message Passing
- Components communicate via `tokio::sync` channels
- Block-aligned processing with `DetectorMessage` enum
- Broadcast channels for fan-out, MPSC for point-to-point

### Trait-Based Extensibility
- `DexAdapter` trait allows pluggable DEX integrations
- `ArbitrageStrategy` trait enables multiple detection algorithms
- Easy to add new DEXes or strategies without core changes

## File Naming Conventions

### Rust Files
- `lib.rs`: Public API and re-exports for each crate
- `main.rs`: Binary entry points
- `mod.rs`: Module organization and re-exports
- Snake_case for all file and module names

### Configuration
- `default.yml`: Base configuration template
- Environment-specific configs: `dev.yml`, `prod.yml`, etc.

### Documentation
- `README.md`: Crate-specific documentation
- `architecture.md`: Design documents
- `*.md`: ADRs and technical specifications

## Import Patterns

### Internal Dependencies
```rust
// Within workspace
use common::{types::TokenPair, errors::DetectorError};
use detector::service::DetectorService;
```

### External Dependencies
```rust
// Standard async patterns
use tokio::sync::{mpsc, broadcast};
use futures::StreamExt;

// Financial calculations
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
```

## Testing Structure
- Unit tests: `#[cfg(test)]` modules in same file
- Integration tests: `tests/` directory in each crate
- Test utilities: `common` crate with `aptos-tests` feature
- Mock data: `recordings/` directory for replay testing

## Development Workflow
1. Changes should be made in appropriate crate based on functionality
2. Update `Cargo.toml` dependencies when adding new crates
3. Run `cargo test` from workspace root to test all crates
4. Use `cargo clippy --deny warnings` to enforce code quality
5. Configuration changes go in `config/` directory, not hardcoded