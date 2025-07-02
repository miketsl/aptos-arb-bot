# DEX Adapters Refactoring Plan

## 1. Objective

The goal of this refactoring is to align the `dex-adapters` crate with the principles outlined in the `architecture.md` document. This involves transforming the adapters from stateful, complex components into simple, stateless translators.

## 2. Core Tasks

The refactoring process is broken down into the following sequential tasks. Each step must be completed before moving to the next to ensure a smooth transition.

### Task 1: Remove the Erroneous `types.rs` File

The file `crates/dex-adapters/src/types.rs` introduces incorrect abstractions and duplicates state that belongs in the `detector` crate. It is the primary source of the architectural issue.

**Action:**
*   Delete the file `crates/dex-adapters/src/types.rs` from the project.

### Task 2: Clean Up the Crate's `lib.rs`

The `lib.rs` file currently exports the incorrect types from the deleted `types.rs`. This must be cleaned up.

**Action:**
*   Open `crates/dex-adapters/src/lib.rs`.
*   Remove the `mod types;` declaration.
*   Remove the `pub use types::{...};` block.

The resulting file should only contain the `mod` declarations for each adapter and the `pub use` statements for the adapter structs themselves.

### Task 3: Refactor `hyperion.rs`

This adapter will be the first to be refactored to the new stateless model.

**Actions:**

1.  **Remove State**:
    *   Delete the `pools: Arc<DashMap<String, PoolState>>` field from the `HyperionAdapter` struct.
    *   The struct should become a zero-sized `struct HyperionAdapter;`.
    *   Remove the `new()` function and replace it with `#[derive(Default)]`.
2.  **Remove State-Management Functions**:
    *   Delete the `get_or_initialize_pool`, `update_pool`, and `track_event_id` methods. They are no longer needed.
3.  **Simplify `parse_event`**:
    *   The function's sole responsibility is to translate an `Event` into a `MarketUpdate`.
    *   Define a new, local struct inside `hyperion.rs` to represent the specific data structure of a Hyperion swap event. This struct should deserialize *all* necessary fields from the `event.data` payload, including the final `sqrt_price`, `liquidity`, `tick`, `fee_bps`, and token identifiers.
    *   The implementation should look conceptually like this:

    ```rust
    // In crates/dex-adapters/src/hyperion.rs
    use anyhow::Result;
    use async_trait::async_trait;
    use common::types::{ClmmMarketUpdate, Event, MarketUpdate, TokenPair};
    use dex_adapter_trait::DexAdapter;
    use serde::Deserialize;
    use std::collections::HashMap;

    // 1. Define a local struct for deserialization
    #[derive(Deserialize)]
    struct HyperionSwapEvent {
        pool_id: String,
        token_a: String,
        token_b: String,
        sqrt_price_after: u128,
        liquidity_after: u128,
        tick_after: i32,
        fee_rate: u32,
    }

    // 2. Make the adapter a zero-sized struct
    #[derive(Default)]
    pub struct HyperionAdapter;

    #[async_trait]
    impl DexAdapter for HyperionAdapter {
        fn id(&self) -> &'static str { "hyperion" }

        // 3. Implement the stateless parsing logic
        fn parse_event(&self, event: &Event) -> Result<Option<MarketUpdate>> {
            let swap: HyperionSwapEvent = match serde_json::from_slice(&event.data) {
                Ok(s) => s,
                Err(_) => return Ok(None),
            };

            let market_update = MarketUpdate::Clmm(ClmmMarketUpdate {
                pool_address: swap.pool_id,
                dex_name: self.id().to_string(),
                token_pair: TokenPair {
                    token0: swap.token_a,
                    token1: swap.token_b,
                },
                sqrt_price: swap.sqrt_price_after,
                liquidity: swap.liquidity_after,
                tick: swap.tick_after,
                fee_bps: swap.fee_rate,
                tick_map: HashMap::new(),
            });

            Ok(Some(market_update))
        }
    }
    ```

### Task 4: Refactor `tapp.rs` and `thala.rs`

**Action:**
*   Apply the exact same refactoring pattern to `tapp.rs` and `thala.rs` as was done for `hyperion.rs`. Each will need its own local struct for deserializing its specific event format, and its `parse_event` function will be reduced to a simple, stateless translation.

### Task 5: Final Compilation and Review

**Action:**
*   After refactoring all adapters, run `cargo check` or `cargo build` within the workspace.
*   Fix any resulting compiler errors. The errors should be confined to the `dex-adapters` crate and will be due to the removal of the `types.rs` file and the change in the adapters' structure.
*   Once the crate compiles, the refactoring is complete. The system is now aligned with the specified architecture.