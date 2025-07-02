# Arbitrage Detector Service

## Overview

The `detector` service is a core component of the arbitrage bot, responsible for identifying and qualifying arbitrage opportunities from a continuous stream of real-time market data. It consumes market updates, maintains a comprehensive price graph of available liquidity pools, and applies various configurable strategies to detect profitable trading paths.

When a potential arbitrage is found, the service enriches it with essential details and forwards it to the risk management and execution services for further processing.

## Architecture

The detector is built around a set of key components that work together to process data and find opportunities efficiently.

-   **`DetectorService`**: The main orchestrator of the crate. It manages the lifecycle of other components, including receiving messages, updating the graph, running strategies, and sending out opportunities. It operates on a block-by-block basis, processing all updates within a block before triggering detection.

-   **`PriceGraph`**: A directed graph representing the entire market state known to the bot.
    -   **Nodes**: Each node in the graph is a unique financial asset (e.g., `USDC`, `APT`), identified by a unique `AssetId`.
    -   **Edges**: Each edge represents a potential trade between two assets. The edge weight contains critical information, including the `PoolModel`, the specific `exchange`, the pool's on-chain `pool_address`, and the latest reserve/liquidity data.

-   **`PoolModel` Enum**: A crucial abstraction that allows the `PriceGraph` to handle various decentralized exchange (DEX) pool types. Each variant of the enum holds the specific parameters required to calculate trade quotes for that model. This design makes the detector extensible to new DEX designs.

-   **`ArbitrageStrategy` Trait**: Defines a common interface for all arbitrage detection algorithms. This trait allows strategies to be developed and tested independently and then plugged into the `DetectorService`. Each strategy specifies the `GraphView` it requires, enabling optimizations where only relevant subgraphs are processed.

## Data Flow

The data flows through the service in a structured, block-aligned sequence:

1.  **Receive Messages**: The `DetectorService` listens for `DetectorMessage`s on a Tokio channel. These messages are sent by the Market Data Ingestor (MDI).
2.  **Block-Scoped Processing**:
    -   `DetectorMessage::BlockStart`: Signals the beginning of a new block, and the service clears its set of updated pairs for the current block.
    -   `DetectorMessage::MarketUpdate`: Contains fresh data for a specific liquidity pool (e.g., `ClmmMarketUpdate`, `ConstantProductMarketUpdate`).
3.  **Transform and Update Graph**:
    -   Each `MarketUpdate` is passed to the `transform_update` function, which converts the raw data into a standardized `Edge` struct.
    -   The `PriceGraph` is updated with this new edge. If an edge for that pool already exists, its data is overwritten with the new information.
4.  **Detect Opportunities**:
    -   `DetectorMessage::BlockEnd`: Signals that all updates for the current block have been received.
    -   The `DetectorService` iterates through its configured `ArbitrageStrategy` implementations.
    -   For each strategy, it creates the required `PriceGraphView` (a snapshot or filtered view of the graph) and invokes the `detect_opportunities` method.
    -   Strategies run in parallel to maximize throughput.
5.  **Deduplicate and Send**:
    -   Detected opportunities are passed through an `OpportunityDeduplicator` to prevent sending redundant signals for the same path in a short time frame.
    -   Unique, profitable `ArbitrageOpportunity` structs are sent to the next service in the pipeline (typically the risk manager) via another Tokio channel.
6.  **Prune Graph**: After detection, the `PriceGraph` is pruned to remove stale edges that have not been updated recently, keeping the graph size manageable and relevant.

## Supported Pool Models

The `PoolModel` enum allows the service to support a variety of on-chain liquidity pool contracts. The current implementations include:

-   **`ConstantProduct`**: Standard Uniswap V2-style pools (`x * y = k`).
-   **`Clmm` (Concentrated Liquidity Market Maker)**: More complex pools like Uniswap V3, where liquidity is concentrated within specific price ranges.
-   **`StableSwap`**: Pools designed for low-slippage trades between similarly priced assets (e.g., Curve).
-   **`WeightedPool`**: Pools with more than two assets or with non-standard weightings (e.g., Balancer).

## Arbitrage Strategies

Arbitrage strategies are implemented as structs that fulfill the `ArbitrageStrategy` trait. This modular approach allows for easy addition of new detection methods.

-   **`TriangularArbitrage`**: Detects 3-hop cycles within the `PriceGraph` (e.g., `A -> B -> C -> A`). It can be configured to run across all DEXes or be limited to a single `target_dex`.
-   **`CrossDexArbitrage`**: Identifies price discrepancies for the same asset pair between two different DEXes. It finds opportunities by buying an asset on one exchange and immediately selling it for a higher price on another (e.g., `Buy USDC/APT on DEX_1`, `Sell USDC/APT on DEX_2`).

## Configuration

Strategies are configured and loaded into the `DetectorService` at startup. The configuration specifies which strategies to run and allows for strategy-specific parameters, such as the `target_dex` for `TriangularArbitrage`. This provides operators with the flexibility to tailor the bot's behavior to different market conditions or objectives.