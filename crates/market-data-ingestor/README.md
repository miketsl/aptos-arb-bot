# Market Data Ingestor

This crate implements a configurable, in-memory market data ingestor for Aptos DEXs using the Aptos Indexer Processor SDK. It monitors on-chain events and pushes parsed market updates directly to the detector component.

## Features

- **No Database**: Operates entirely in-memory without any database dependencies
- **Configurable**: Can monitor specific DEXs, events, and pools via configuration
- **CLMM Support**: Supports Concentrated Liquidity Market Maker DEXs like Hyperion and ThalaSwap
- **Real-time Updates**: Processes swap events in real-time and maintains pool state
- **Built on Aptos SDK**: Uses the official Aptos Indexer Processor SDK for reliable event streaming

## Architecture

The ingestor uses a pipeline architecture:

1. **Transaction Stream**: Connects to Aptos gRPC endpoint to receive transactions
2. **Event Extraction**: Filters blockchain transactions for relevant DEX events
3. **CLMM Parser**: Maintains pool state and parses events into market updates
4. **Detector Push**: Sends updates to the detector via an in-memory channel

## Configuration

Configure DEXs in `config/default.yml`:

```yaml
market_data_ingestor:
  processor_config:
    type: market_data_ingestor
  transaction_stream_config:
    starting_version: null  # null means start from latest
    grpc_data_stream_endpoint: "https://grpc.mainnet.aptoslabs.com:443"
    grpc_auth_token: "YOUR_API_KEY_HERE"
  market_data_config:
    dexs:
      - name: "Hyperion"
        module_address: "0x..."
        pool_snapshot_event_name: "0x...::pool::PoolSnapshot"
        swap_event_name: "0x...::pool::SwapAfterEvent"
        pools:
          - "0xpool_address"
```

## Integration

The ingestor integrates with the detector crate by:
1. Converting CLMM pool states into market updates
2. Sending updates via a `tokio::sync::mpsc` channel
3. The detector receives these updates and updates its arbitrage graph

## Running Standalone

For testing purposes, you can run the ingestor standalone:

```bash
cargo run -p market-data-ingestor -- --config-path config/default.yml
```

## Event Processing

The ingestor processes two main types of events:

1. **PoolSnapshot Events**: Used for initialization and reconciliation
   - Contains complete pool state (sqrt_price, liquidity, tick, fee_rate)
   - First snapshot initializes the pool in memory
   - Subsequent snapshots reconcile state

2. **SwapAfterEvent**: Real-time price updates
   - Updates sqrt_price, liquidity, and tick after each swap
   - Generates MarketUpdate for the detector