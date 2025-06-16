//! Prelude for the detector crate.

// Re-export commonly used types and traits from this crate.
pub use crate::graph::{Edge, PoolModel, PriceGraph, PriceGraphSnapshot, Tick};
pub use crate::service::{DexAdapters, PriceStream};
pub use crate::{Detector, DetectorConfig};

// Re-export relevant items from common crate
pub use common::errors::CommonError;
pub use common::types::{Asset, CycleEval, PathQuote, Quantity, TradingPair}; // Or a more specific error type for detector

// Re-export external crates if they are widely used within the detector crate modules
pub use aptos_sdk::types::account_address::AccountAddress;
pub use petgraph;
pub use rust_decimal::Decimal;
pub use std::time::{Duration, Instant}; // Will be used for DiGraphMap

// Placeholder for other common items
// pub use crate::bellman_ford::some_item;
// pub use crate::sizing::some_item;
// pub use crate::gas::some_item;
