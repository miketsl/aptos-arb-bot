# Pool Model Expansion Plan

This document outlines the plan to expand the `PoolModel` in the `detector` crate to support various DEX pool types, including CLMM, AMM (Constant Product), StableSwap, and Weighted Pools. This is crucial for accurate quoting and arbitrage opportunity detection across the diverse Aptos DEX ecosystem.

## Overall Strategy:

1.  **Extend `common::types::MarketUpdate`:** Make it an enum to carry specific data for each pool type.
2.  **Extend `detector::graph::PoolModel`:** Mirror the `MarketUpdate` enum with corresponding variants for the `detector`'s internal representation.
3.  **Update `dex-adapters`:** Modify existing adapters and create new ones to produce the new `MarketUpdate` enum variants.
4.  **Update `detector::transform`:** Adapt the transformation logic to handle the new `MarketUpdate` enum and create the correct `PoolModel` variants.
5.  **Implement Quoting Logic:** Add the mathematical functions for `StableSwap` and `WeightedPool` within the `detector` crate.
6.  **Integrate and Test:** Update arbitrage strategies and write comprehensive tests.

---

## Detailed Plan:

### Phase 0: Extend `common::types::MarketUpdate`

*   **Goal:** Redefine `MarketUpdate` as an enum to encapsulate data specific to each pool type.
*   **File:** `crates/common/src/types.rs`
*   **Changes:**
    *   Define new structs for each pool type's market update data:
        ```rust
        // Existing CLMM-centric data, now explicitly named
        pub struct ClmmMarketUpdate {
            pub pool_address: String,
            pub dex_name: String,
            pub token_pair: TokenPair,
            pub sqrt_price: u128,
            pub liquidity: u128,
            pub tick: i32, // Changed from u32 to i32 to match CLMM spec
            pub fee_bps: u32,
            pub tick_map: HashMap<i32, TickInfo>,
        }

        // For Constant Product Market Makers (CPMM)
        pub struct ConstantProductMarketUpdate {
            pub pool_address: String,
            pub dex_name: String,
            pub token_pair: TokenPair,
            pub reserve_x: Quantity,
            pub reserve_y: Quantity,
            pub fee_bps: u32,
        }

        // For StableSwap pools (e.g., Curve-like)
        pub struct StableSwapMarketUpdate {
            pub pool_address: String,
            pub dex_name: String,
            pub token_pair: TokenPair, // Or Vec<Asset> for multi-asset stableswaps
            pub reserves: Vec<Quantity>, // Current reserves of all assets
            pub amplification_factor: u128, // The 'A' parameter
            pub fee_bps: u32,
            // Add other relevant StableSwap parameters like future_A, future_A_time, etc. if needed
        }

        // For Weighted Pools (e.g., Balancer-like)
        pub struct WeightedPoolMarketUpdate {
            pub pool_address: String,
            pub dex_name: String,
            pub token_pair: TokenPair, // Or Vec<Asset> for multi-asset weighted pools
            pub reserves: Vec<Quantity>, // Current reserves of all assets
            pub weights: Vec<u32>, // Weights for each asset, e.g., 500000 for 50%
            pub fee_bps: u32,
        }
        ```
    *   Redefine `MarketUpdate` as an enum using these new structs:
        ```rust
        pub enum MarketUpdate {
            Clmm(ClmmMarketUpdate),
            ConstantProduct(ConstantProductMarketUpdate),
            StableSwap(StableSwapMarketUpdate),
            WeightedPool(WeightedPoolMarketUpdate),
            // Add other pool types as needed
        }
        ```
    *   **Impact:** This will require updating all existing code that constructs or consumes `MarketUpdate` (e.g., `dex-adapters`, `market-data-ingestor/processor.rs`, `detector/transform.rs`).

### Phase 1: Extend `detector::graph::PoolModel`

*   **Goal:** Introduce `Clmm`, `StableSwap`, and `WeightedPool` variants to the `PoolModel` enum, mirroring the `MarketUpdate` structure.
*   **File:** `crates/detector/src/graph/mod.rs` (or wherever `PoolModel` is defined, likely in `graph/mod.rs` or `graph/state.rs`).
*   **Changes:**
    *   Modify the `PoolModel` enum:
        ```rust
        pub enum PoolModel {
            ConstantProduct {
                reserve_x: Quantity,
                reserve_y: Quantity,
                fee_bps: u32,
            },
            Clmm {
                sqrt_price: u128,
                liquidity: u128,
                tick: i32,
                fee_bps: u32,
                tick_map: Arc<HashMap<i32, TickInfo>>, // Use Arc for efficiency
            },
            StableSwap {
                reserves: Vec<Quantity>,
                amplification_factor: u128,
                fee_bps: u32,
            },
            WeightedPool {
                reserves: Vec<Quantity>,
                weights: Vec<u32>,
                fee_bps: u32,
            },
        }
        ```
    *   Ensure `TickInfo` and `Quantity` are correctly imported or defined.

### Phase 2: Update `dex-adapters` Implementations

*   **Goal:** Modify existing `DexAdapter`s and implement new ones to produce the new `MarketUpdate` enum variants based on the DEX and pool type.
*   **File:** `crates/dex-adapters/src/lib.rs`
*   **Changes:**
    *   **`HyperionAdapter`:** Update `parse_event` to return `MarketUpdate::Clmm(ClmmMarketUpdate { ... })`.
    *   **`ThalaAdapter`:**
        *   Update existing `parse_event` logic to return `MarketUpdate::Clmm(ClmmMarketUpdate { ... })` for CLMM pools.
        *   **Add logic for Weighted Pools:** This will require identifying the specific event types for Thala's weighted pools, parsing their data (reserves, weights, fees), and returning `MarketUpdate::WeightedPool(WeightedPoolMarketUpdate { ... })`. You'll likely need new internal structs (`WeightedPoolSnapshotData`, `WeightedPoolSwapEventData`) to deserialize the raw event data.
    *   **`TappAdapter`:**
        *   **Implement logic for CLMM:** Parse Tapp's CLMM events and return `MarketUpdate::Clmm(...)`.
        *   **Implement logic for AMM (Constant Product):** Parse Tapp's AMM events (reserves, fees) and return `MarketUpdate::ConstantProduct(...)`.
        *   **Implement logic for StableSwap:** Parse Tapp's StableSwap events (reserves, amplification factor, fees) and return `MarketUpdate::StableSwap(...)`. You'll need new internal structs for deserialization.
    *   **General:** Each `DexAdapter`'s `parse_event` method will need to correctly identify the pool type from the event data (e.g., `type_str`, or specific event fields) and construct the appropriate `MarketUpdate` variant.

### Phase 3: Update Data Transformation (`MarketUpdate` to `Edge/PoolModel`)

*   **Goal:** Modify the logic that converts `MarketUpdate`s into `Edge` objects with the correct `PoolModel` variant.
*   **File:** `crates/detector/src/transform.rs` (or the module responsible for this conversion).
*   **Changes:**
    *   The function responsible for creating an `Edge` from a `MarketUpdate` will now need to use a `match` statement on the `MarketUpdate` enum:
        ```rust
        // Example snippet (actual implementation might vary)
        fn market_update_to_edge(update: MarketUpdate) -> Edge {
            match update {
                MarketUpdate::Clmm(data) => Edge {
                    // ... common fields
                    model: PoolModel::Clmm {
                        sqrt_price: data.sqrt_price,
                        liquidity: data.liquidity,
                        tick: data.tick,
                        fee_bps: data.fee_bps,
                        tick_map: Arc::new(data.tick_map), // Wrap in Arc here
                    },
                },
                MarketUpdate::ConstantProduct(data) => Edge {
                    // ... common fields
                    model: PoolModel::ConstantProduct {
                        reserve_x: data.reserve_x,
                        reserve_y: data.reserve_y,
                        fee_bps: data.fee_bps,
                    },
                },
                MarketUpdate::StableSwap(data) => Edge {
                    // ... common fields
                    model: PoolModel::StableSwap {
                        reserves: data.reserves,
                        amplification_factor: data.amplification_factor,
                        fee_bps: data.fee_bps,
                    },
                },
                MarketUpdate::WeightedPool(data) => Edge {
                    // ... common fields
                    model: PoolModel::WeightedPool {
                        reserves: data.reserves,
                        weights: data.weights,
                        fee_bps: data.fee_bps,
                    },
                },
            }
        }
        ```
    *   Ensure `Arc` is used for `tick_map` when creating `PoolModel::Clmm`.

### Phase 4: Implement Quoting Logic for New Pool Models

*   **Goal:** Develop the core mathematical functions for calculating output amounts for `Clmm`, `StableSwap`, and `WeightedPool` models.
*   **File:** `crates/detector/src/graph/mod.rs` (or create new modules like `crates/detector/src/pool_math.rs` and put the logic there, then import).
*   **Changes:**
    *   Add a method (e.g., `get_output_amount`) to the `PoolModel` enum (or implement a trait for it). This method will take an input amount and the direction of the trade, and calculate the estimated output amount.
    *   **`PoolModel::Clmm`:** Implement the Uniswap V3-style concentrated liquidity math using `sqrt_price`, `liquidity`, `tick`, `fee_bps`, and `tick_map`. This is the most complex part.
    *   **`PoolModel::StableSwap`:** Implement the StableSwap (e.g., Curve-like) math using `reserves`, `amplification_factor`, and `fee_bps`.
    *   **`PoolModel::WeightedPool`:** Implement the weighted pool math using `reserves`, `weights`, and `fee_bps`.
    *   **`PoolModel::ConstantProduct`:** Ensure its `get_output_amount` is correctly implemented.

### Phase 5: Integrate into Arbitrage Strategies

*   **Goal:** Update the existing arbitrage strategies to utilize the new `PoolModel` quoting logic.
*   **Files:** `crates/detector/src/strategies/cross_dex.rs`, `crates/detector/src/strategies/triangular.rs`
*   **Changes:**
    *   Modify the profit calculation logic within these strategies. They should now call the generic `get_output_amount` method on the `PoolModel` variant of each `Edge` in the path. This method will internally dispatch to the correct calculation based on the `PoolModel` type.

### Phase 6: Comprehensive Testing

*   **Goal:** Ensure the new quoting logic is correct and the integration is stable.
*   **Files:** `crates/detector/tests/` (create new test files as needed, e.g., `clmm_quoting_tests.rs`, `stableswap_tests.rs`, `weighted_pool_tests.rs`).
*   **Changes:**
    *   Write unit tests for each new `PoolModel`'s `get_output_amount` method, covering various input amounts, pool states, and edge cases.
    *   Add integration tests that simulate `MarketUpdate`s for CLMM, StableSwap, and Weighted Pools, and verify that the `detector` correctly identifies and quantifies arbitrage opportunities involving these pools.
    *   Ensure existing tests for `ConstantProduct` pools still pass.
