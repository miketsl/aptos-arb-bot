//! # Aptos Arb Bot Common Crate
//!
//! This crate provides common data types, error definitions, and utility functions
//! used across the `aptos-arb-bot` workspace.

/// Module for common error types.
pub mod errors;

/// Module for common data structures and types.
pub mod types;

// Re-export key items for easier access.
pub use errors::CommonError;
pub use types::{Asset, AssetPair, ExchangeId, Price, Quantity};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_re_exports_exist() {
        // This test primarily ensures that the re-exported items are accessible.
        // If this compiles, the re-exports are working.
        let _asset = Asset("TEST".to_string());
        let _price = Price(rust_decimal_macros::dec!(1.0));
        let _quantity = Quantity(rust_decimal_macros::dec!(100.0));
        let _exchange_id = ExchangeId("EXCHANGE_TEST".to_string());
        let _asset_pair = AssetPair {
            base: Asset("BASE".to_string()),
            quote: Asset("QUOTE".to_string()),
        };
        let _err = CommonError::NotFound("test".to_string());
    }
}
