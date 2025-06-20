//! Exchange constants and helpers for the detector module.

use std::fmt;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Exchange {
    Tapp,
    PancakeSwap,
    Thala,
    Hyperion,
}

impl Exchange {
    pub fn as_str(&self) -> &'static str {
        match self {
            Exchange::Tapp => "Tapp",
            Exchange::PancakeSwap => "PancakeSwap",
            Exchange::Thala => "Thala",
            Exchange::Hyperion => "Hyperion",
        }
    }
}

impl std::str::FromStr for Exchange {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Tapp" => Ok(Exchange::Tapp),
            "PancakeSwap" => Ok(Exchange::PancakeSwap),
            "Thala" => Ok(Exchange::Thala),
            "Hyperion" => Ok(Exchange::Hyperion),
            _ => Err(anyhow::anyhow!("Invalid exchange: {}", s)),
        }
    }
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

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
