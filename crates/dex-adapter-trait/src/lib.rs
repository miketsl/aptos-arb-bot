use common::types::Asset;
use async_trait::async_trait;
use std::error::Error;
use std::fmt;

/// Represents a pair of assets for trading.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TradingPair(pub Asset, pub Asset);

impl TradingPair {
    /// Creates a new trading pair.
    pub fn new(asset1: Asset, asset2: Asset) -> Self {
        TradingPair(asset1, asset2)
    }
}

impl fmt::Display for TradingPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.0, self.1)
    }
}

/// Represents a price quote for a trading pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    /// The amount of the output asset that will be received.
    pub amount_out: u64,
    /// The asset that will be received.
    pub asset_out: Asset,
    /// Optional price impact of the trade.
    pub price_impact: Option<f64>,
}

/// Represents the result of a swap operation.
#[derive(Debug, Clone, PartialEq)]
pub struct SwapResult {
    /// The unique identifier of the transaction.
    pub transaction_id: String,
    /// The amount of the output asset received from the swap.
    pub amount_out_received: u64,
}

/// Errors that can occur when interacting with a DEX adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum DexAdapterError {
    /// Error related to network communication.
    NetworkError(String),
    /// The requested trading pair was not found or is not supported.
    PairNotFound(TradingPair),
    /// Insufficient liquidity in the pool to perform the requested operation.
    InsufficientLiquidity(TradingPair),
    /// An underlying error from the DEX or a library.
    UnderlyingError(String),
    /// An error occurred while preparing or sending the transaction.
    TransactionFailed(String),
    /// Configuration error for the adapter.
    ConfigurationError(String),
    /// An unknown or unspecified error.
    Unknown(String),
}

impl fmt::Display for DexAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DexAdapterError::NetworkError(s) => write!(f, "Network error: {}", s),
            DexAdapterError::PairNotFound(pair) => write!(f, "Pair not found: {}", pair),
            DexAdapterError::InsufficientLiquidity(pair) => {
                write!(f, "Insufficient liquidity for pair: {}", pair)
            }
            DexAdapterError::UnderlyingError(s) => write!(f, "Underlying error: {}", s),
            DexAdapterError::TransactionFailed(s) => write!(f, "Transaction failed: {}", s),
            DexAdapterError::ConfigurationError(s) => write!(f, "Configuration error: {}", s),
            DexAdapterError::Unknown(s) => write!(f, "Unknown error: {}", s),
        }
    }
}

impl Error for DexAdapterError {}

/// A trait defining the interface for interacting with a Decentralized Exchange (DEX).
///
/// This trait provides a standardized way to get information about supported trading pairs,
/// obtain quotes for swaps, and execute swaps.
#[async_trait]
pub trait DexAdapter: Send + Sync {
    /// Retrieves a list of all trading pairs supported by the DEX.
    ///
    /// # Returns
    /// A `Result` containing a `Vec` of `TradingPair`s on success, or a `DexAdapterError` on failure.
    async fn get_supported_pairs(&self) -> Result<Vec<TradingPair>, DexAdapterError>;

    /// Gets a quote for swapping a specific amount of an input asset for an output asset
    /// within a given trading pair.
    ///
    /// # Arguments
    /// * `pair` - A reference to the `TradingPair` for which the quote is requested.
    /// * `amount_in` - The amount of the input asset to be swapped.
    /// * `asset_in` - A reference to the `Asset` being offered.
    ///
    /// # Returns
    /// A `Result` containing a `Quote` on success, or a `DexAdapterError` on failure.
    /// The `asset_out` in the `Quote` will be the other asset in the `pair`.
    async fn get_quote(
        &self,
        pair: &TradingPair,
        amount_in: u64,
        asset_in: &Asset,
    ) -> Result<Quote, DexAdapterError>;

    /// Executes a swap on the DEX.
    ///
    /// # Arguments
    /// * `pair` - A reference to the `TradingPair` for the swap.
    /// * `amount_in` - The amount of the input asset to be swapped.
    /// * `asset_in` - A reference to the `Asset` being offered.
    /// * `min_amount_out` - The minimum amount of the output asset that must be received for
    ///   the swap to be considered successful (slippage protection).
    ///
    /// # Returns
    /// A `Result` containing a `SwapResult` on successful execution, or a `DexAdapterError` on failure.
    async fn swap(
        &self,
        pair: &TradingPair,
        amount_in: u64,
        asset_in: &Asset,
        min_amount_out: u64,
    ) -> Result<SwapResult, DexAdapterError>;
}

/// A mock implementation of the `DexAdapter` trait for testing purposes.
///
/// This adapter simulates the behavior of a real DEX adapter with a predefined set of
/// trading pairs and simple logic for quotes and swaps.
#[derive(Default)]
pub struct MockDexAdapter {
    supported_pairs: Vec<TradingPair>,
}

impl MockDexAdapter {
    /// Creates a new `MockDexAdapter` with default mock data.
    ///
    /// The default data includes pairs like (APT, USDC) and (MOJO, APT).
    pub fn new() -> Self {
        MockDexAdapter {
            supported_pairs: vec![
                TradingPair::new(Asset::from("APT"), Asset::from("USDC")),
                TradingPair::new(Asset::from("MOJO"), Asset::from("APT")),
                TradingPair::new(Asset::from("USDT"), Asset::from("USDC")),
            ],
        }
    }

    /// Creates a `MockDexAdapter` with a specific list of supported pairs.
    #[allow(dead_code)] // May be used in other tests
    pub fn with_pairs(pairs: Vec<TradingPair>) -> Self {
        MockDexAdapter {
            supported_pairs: pairs,
        }
    }
}

#[async_trait]
impl DexAdapter for MockDexAdapter {
    /// Returns the predefined list of supported trading pairs.
    async fn get_supported_pairs(&self) -> Result<Vec<TradingPair>, DexAdapterError> {
        Ok(self.supported_pairs.clone())
    }

    /// Returns a mock quote based on a hardcoded ratio.
    ///
    /// For example, if APT/USDC is queried with APT as input, it might return 10 USDC per APT.
    /// It ensures `asset_out` is the other asset in the pair.
    async fn get_quote(
        &self,
        pair: &TradingPair,
        amount_in: u64,
        asset_in: &Asset,
    ) -> Result<Quote, DexAdapterError> {
        if !self.supported_pairs.contains(pair) && !self.supported_pairs.contains(&TradingPair(pair.1.clone(), pair.0.clone())) {
            return Err(DexAdapterError::PairNotFound(pair.clone()));
        }

        let (asset1, asset2) = (&pair.0, &pair.1);
        let (expected_asset_out, ratio) = if asset_in == asset1 {
            (asset2.clone(), 10.0) // e.g., 1 asset1 = 10 asset2
        } else if asset_in == asset2 {
            (asset1.clone(), 0.1) // e.g., 1 asset2 = 0.1 asset1
        } else {
            // This case should ideally not be reached if pair contains asset_in
            return Err(DexAdapterError::UnderlyingError(format!(
                "Asset {} not found in pair {}",
                asset_in, pair
            )));
        };

        let amount_out = (amount_in as f64 * ratio) as u64;

        Ok(Quote {
            amount_out,
            asset_out: expected_asset_out,
            price_impact: Some(0.01), // Mock price impact: 1%
        })
    }

    /// Simulates a successful swap and returns a mock transaction ID.
    ///
    /// The `amount_out_received` is based on the mock quote logic.
    async fn swap(
        &self,
        pair: &TradingPair,
        amount_in: u64,
        asset_in: &Asset,
        min_amount_out: u64,
    ) -> Result<SwapResult, DexAdapterError> {
        let quote = self.get_quote(pair, amount_in, asset_in).await?;

        if quote.amount_out < min_amount_out {
            return Err(DexAdapterError::InsufficientLiquidity(pair.clone())); // Or a more specific error
        }

        Ok(SwapResult {
            transaction_id: format!("mock_tx_{}", rand::random::<u32>()),
            amount_out_received: quote.amount_out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::Asset;

    fn apt() -> Asset { Asset::from("APT") }
    fn usdc() -> Asset { Asset::from("USDC") }
    fn mojo() -> Asset { Asset::from("MOJO") }
    fn usdt() -> Asset { Asset::from("USDT") }


    #[tokio::test]
    async fn mock_adapter_get_supported_pairs() {
        let adapter = MockDexAdapter::new();
        let pairs = adapter.get_supported_pairs().await.unwrap();
        assert_eq!(pairs.len(), 3);
        assert!(pairs.contains(&TradingPair::new(apt(), usdc())));
        assert!(pairs.contains(&TradingPair::new(mojo(), apt())));
        assert!(pairs.contains(&TradingPair::new(usdt(), usdc())));
    }

    #[tokio::test]
    async fn mock_adapter_get_quote_apt_usdc() {
        let adapter = MockDexAdapter::new();
        let pair = TradingPair::new(apt(), usdc());
        let amount_in: u64 = 100; // 100 APT
        let quote = adapter.get_quote(&pair, amount_in, &apt()).await.unwrap();

        assert_eq!(quote.asset_out, usdc());
        assert_eq!(quote.amount_out, 1000); // 100 APT * 10 = 1000 USDC
        assert_eq!(quote.price_impact, Some(0.01));
    }

    #[tokio::test]
    async fn mock_adapter_get_quote_usdc_apt() {
        let adapter = MockDexAdapter::new();
        let pair = TradingPair::new(apt(), usdc()); // Order in pair definition doesn't strictly matter for mock
        let amount_in: u64 = 1000; // 1000 USDC
        let quote = adapter.get_quote(&pair, amount_in, &usdc()).await.unwrap();

        assert_eq!(quote.asset_out, apt());
        assert_eq!(quote.amount_out, 100); // 1000 USDC * 0.1 = 100 APT
    }

    #[tokio::test]
    async fn mock_adapter_get_quote_pair_not_found() {
        let adapter = MockDexAdapter::new();
        let pair = TradingPair::new(Asset::from("UNKNOWN"), Asset::from("TOKEN"));
        let result = adapter.get_quote(&pair, 100, &Asset::from("UNKNOWN")).await;
        assert!(matches!(result, Err(DexAdapterError::PairNotFound(_))));
        if let Err(DexAdapterError::PairNotFound(reported_pair)) = result {
            assert_eq!(reported_pair, pair);
        }
    }
     #[tokio::test]
    async fn mock_adapter_get_quote_asset_not_in_pair() {
        let adapter = MockDexAdapter::new();
        let pair = TradingPair::new(apt(), usdc());
        // Try to get a quote for MOJO using the APT/USDC pair
        let result = adapter.get_quote(&pair, 100, &mojo()).await;
        assert!(matches!(result, Err(DexAdapterError::UnderlyingError(_))));
         if let Err(DexAdapterError::UnderlyingError(msg)) = result {
            assert!(msg.contains("Asset MOJO not found in pair APT/USDC"));
        }
    }


    #[tokio::test]
    async fn mock_adapter_swap_successful() {
        let adapter = MockDexAdapter::new();
        let pair = TradingPair::new(apt(), usdc());
        let amount_in: u64 = 50; // 50 APT
        let min_amount_out: u64 = 490; // Expect at least 490 USDC

        let result = adapter.swap(&pair, amount_in, &apt(), min_amount_out).await.unwrap();

        assert!(result.transaction_id.starts_with("mock_tx_"));
        assert_eq!(result.amount_out_received, 500); // 50 APT * 10 = 500 USDC
    }

    #[tokio::test]
    async fn mock_adapter_swap_insufficient_output() {
        let adapter = MockDexAdapter::new();
        let pair = TradingPair::new(apt(), usdc());
        let amount_in: u64 = 50; // 50 APT
        // Expect at least 501 USDC, but mock quote will give 500
        let min_amount_out: u64 = 501;

        let result = adapter.swap(&pair, amount_in, &apt(), min_amount_out).await;
        assert!(matches!(result, Err(DexAdapterError::InsufficientLiquidity(_))));
         if let Err(DexAdapterError::InsufficientLiquidity(reported_pair)) = result {
            assert_eq!(reported_pair, pair);
        }
    }

    #[tokio::test]
    async fn mock_adapter_swap_pair_not_found() {
        let adapter = MockDexAdapter::new();
        let pair = TradingPair::new(Asset::from("FAKE"), Asset::from("COIN"));
        let result = adapter.swap(&pair, 100, &Asset::from("FAKE"), 90).await;
        assert!(matches!(result, Err(DexAdapterError::PairNotFound(_))));
    }
}
