# Detector Service

The detector service is responsible for detecting arbitrage opportunities from a stream of market data. It sits in the middle of the arbitrage bot pipeline, receiving market updates from the Market Data Ingestor (MDI) and publishing opportunities to the Risk Manager.

## Architecture

The detector follows a modern async service architecture:

- **Input**: Receives `DetectorMessage` events via tokio broadcast channels from MDI
- **Processing**: Maintains a unified price graph and runs multiple arbitrage strategies in parallel
- **Output**: Publishes `ArbitrageOpportunity` events via tokio mpsc channels to Risk Manager

### Key Components

- **DetectorService**: Main service orchestrating the detection pipeline
- **PriceGraph**: Unified graph storing market data from all DEXes using petgraph
- **Strategies**: Pluggable arbitrage detection algorithms (CrossDex, Triangular, MultiHop)
- **Transform Layer**: Converts market updates to graph edges
- **Deduplicator**: Prevents duplicate opportunities from being published
- **Smart Pruning**: Maintains optimal graph size based on activity and TVL

## Usage

```rust
use detector::{DetectorService, strategies::StrategyConfig};
use tokio::sync::mpsc;

// Create channels
let (tx, rx) = mpsc::channel(100);
let (opportunity_tx, opportunity_rx) = mpsc::channel(100);

// Configure strategies
let strategies = vec![
    StrategyConfig::CrossDex(Default::default()),
    StrategyConfig::Triangular(detector::strategies::TriangularConfig {
        max_path_length: 3,
        target_dex: None,
    }),
];

// Create and run service
let service = DetectorService::new(rx, opportunity_tx, strategies)?;
service.run().await?;
```

## Message Flow

1. **BlockStart**: Clears state for new block processing
2. **MarketUpdate**: Transforms update to graph edge and updates price graph
3. **BlockEnd**: Runs all strategies in parallel and publishes opportunities

## Strategies

### CrossDex Arbitrage
Detects price differences for the same trading pair across different DEXes.

### Triangular Arbitrage  
Finds profitable cycles within a single DEX (e.g., A→B→C→A).

### MultiHop Arbitrage
Discovers longer arbitrage paths across multiple hops and DEXes.

## Performance

- **Latency**: < 10ms per block detection cycle
- **Memory**: < 1GB for 10,000 edges  
- **Parallel Execution**: Each strategy runs in its own async task
- **Smart Pruning**: Maintains optimal graph size automatically

## Testing

```bash
# Run unit tests
cargo test --lib

# Run integration tests  
cargo test --tests

# Run all tests
cargo test
```

## Configuration

The detector supports runtime configuration via YAML:

```yaml
detector:
  strategies:
    - type: cross_dex
      enabled: true
    - type: triangular_arbitrage
      enabled: true
      config:
        max_path_length: 3
  graph:
    pruning:
      opportunity_window_minutes: 60
      min_tvl_usd: 1000
```

See `architecture.md` for detailed design documentation.