//! Exchange constants and helpers for the detector module.

pub use dex_adapter_trait::Exchange;

/// Creates a test exchange for use in unit tests.
pub fn test_exchange() -> Exchange {
    Exchange::Tapp
}

/// Creates a mock exchange for use in examples and demonstrations.
pub fn mock_exchange() -> Exchange {
    Exchange::Tapp
}

/// Returns a default exchange for testing purposes.
pub const fn default_test_exchange() -> Exchange {
    Exchange::Tapp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_helpers() {
        assert_eq!(test_exchange(), Exchange::Tapp);
        assert_eq!(mock_exchange(), Exchange::Tapp);
        assert_eq!(default_test_exchange(), Exchange::Tapp);
    }
}
