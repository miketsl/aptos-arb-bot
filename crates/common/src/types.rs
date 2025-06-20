use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Represents a price, typically using a high-precision decimal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(pub Decimal);

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a quantity of an asset, typically using a high-precision decimal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quantity(pub Decimal);

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a financial asset, identified by a symbol string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Asset(pub String);

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for Asset {
    fn from(s: &str) -> Self {
        Asset(s.to_uppercase())
    }
}

impl std::str::FromStr for Asset {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Asset(s.to_string()))
    }
}

/// Represents a pair of assets for trading.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetPair {
    /// The base asset of the pair.
    pub base: Asset,
    /// The quote asset of the pair.
    pub quote: Asset,
}

impl AssetPair {
    /// Creates a new asset pair.
    pub fn new(base: Asset, quote: Asset) -> Self {
        AssetPair { base, quote }
    }
}

impl fmt::Display for AssetPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

/// Represents a trading pair between two assets.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradingPair {
    pub asset_x: Asset,
    pub asset_y: Asset,
}

impl TradingPair {
    pub fn new(asset_x: Asset, asset_y: Asset) -> Self {
        TradingPair { asset_x, asset_y }
    }
}

impl fmt::Display for TradingPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.asset_x, self.asset_y)
    }
}

/// Represents the type of an order (Buy or Sell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderType {
    Buy,
    Sell,
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderType::Buy => write!(f, "BUY"),
            OrderType::Sell => write!(f, "SELL"),
        }
    }
}

/// Represents a trade order.
#[derive(Debug, Clone, PartialEq)]
pub struct Order<E> {
    pub id: String, // Unique order identifier
    pub pair: AssetPair,
    pub order_type: OrderType,
    pub price: Price,
    pub quantity: Quantity,
    pub exchange: E,
    // Timestamp, etc. can be added later
}

/// Represents the status of a trade execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TradeStatus {
    Pending,
    Filled,
    PartiallyFilled,
    Cancelled,
    Rejected,
    Error,
}

impl fmt::Display for TradeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Represents the result of a trade execution attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeResult {
    pub order_id: String,
    pub status: TradeStatus,
    pub filled_quantity: Quantity,
    pub filled_price: Option<Price>, // Average filled price
    pub message: Option<String>,     // For errors or additional info
}

/// Represents a path quote result from the arbitrage detector.
#[derive(Debug, Clone, PartialEq)]
pub struct PathQuote<E> {
    pub path: Vec<(Asset, E)>,
    pub amount_in: Quantity,
    pub amount_out: Quantity,
    /// Profit percentage, expressed as a fraction (e.g., 0.01 for 1%).
    pub profit_pct: f64,
}

/// Represents cycle evaluation with gas accounting.
#[derive(Debug, Clone, PartialEq)]
pub struct CycleEval {
    pub gross_profit: rust_decimal::Decimal,
    pub gas_estimate: u64,
    pub gas_unit_price: rust_decimal::Decimal,
    pub net_profit: rust_decimal::Decimal,
}

/// Token pair for CLMM pools
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TokenPair {
    pub token0: String,
    pub token1: String,
}

/// Information about a specific tick in the CLMM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickInfo {
    pub liquidity_net: i128,
    pub liquidity_gross: u128,
}

/// Market update to be sent to the detector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketUpdate {
    pub pool_address: String,
    pub dex_name: String,
    pub token_pair: TokenPair,
    pub sqrt_price: u128,
    pub liquidity: u128,
    pub tick: u32,
    pub fee_bps: u32,
    pub tick_map: HashMap<i32, TickInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_price_display() {
        let price = Price(dec!(123.45));
        assert_eq!(format!("{}", price), "123.45");
    }

    #[test]
    fn test_quantity_display() {
        let quantity = Quantity(dec!(0.5));
        assert_eq!(format!("{}", quantity), "0.5");
    }

    #[test]
    fn test_asset_display_and_from_str() {
        let asset = Asset::from("btc");
        assert_eq!(asset, Asset("BTC".to_string()));
        assert_eq!(format!("{}", asset), "BTC");
    }

    #[test]
    fn test_asset_pair_new_and_display() {
        let base = Asset::from("eth");
        let quote = Asset::from("usdt");
        let pair = AssetPair::new(base.clone(), quote.clone());
        assert_eq!(pair.base, base);
        assert_eq!(pair.quote, quote);
        assert_eq!(format!("{}", pair), "ETH/USDT");
    }

    #[test]
    fn test_asset_pair_ordering() {
        let pair1 = AssetPair::new(Asset::from("btc"), Asset::from("usdt"));
        let pair2 = AssetPair::new(Asset::from("eth"), Asset::from("usdt"));
        let pair3 = AssetPair::new(Asset::from("btc"), Asset::from("eth"));
        assert!(pair1 < pair2); // BTC < ETH
        assert!(pair3 < pair1); // ETH < USDT (when base is same)
    }

    #[test]
    fn test_price_ordering() {
        let price1 = Price(dec!(100.0));
        let price2 = Price(dec!(200.0));
        assert!(price1 < price2);
    }

    #[test]
    fn test_quantity_ordering() {
        let q1 = Quantity(dec!(10.0));
        let q2 = Quantity(dec!(5.0));
        assert!(q2 < q1);
    }

    #[test]
    fn test_order_type_display() {
        assert_eq!(format!("{}", OrderType::Buy), "BUY");
        assert_eq!(format!("{}", OrderType::Sell), "SELL");
    }

    #[test]
    fn test_trade_status_display() {
        assert_eq!(format!("{}", TradeStatus::Filled), "Filled");
        assert_eq!(format!("{}", TradeStatus::Error), "Error");
    }

    #[test]
    fn test_order_creation() {
        let order = Order {
            id: "order123".to_string(),
            pair: AssetPair::new(Asset::from("BTC"), Asset::from("USDT")),
            order_type: OrderType::Buy,
            price: Price(dec!(50000.0)),
            quantity: Quantity(dec!(0.5)),
            exchange: "test-exchange",
        };
        assert_eq!(order.id, "order123");
        assert_eq!(order.order_type, OrderType::Buy);
        assert_eq!(order.exchange, "test-exchange");
    }

    #[test]
    fn test_trade_result_creation() {
        let result = TradeResult {
            order_id: "order123".to_string(),
            status: TradeStatus::Filled,
            filled_quantity: Quantity(dec!(0.5)),
            filled_price: Some(Price(dec!(50000.0))),
            message: None,
        };
        assert_eq!(result.status, TradeStatus::Filled);
        assert_eq!(result.filled_quantity, Quantity(dec!(0.5)));
    }
}

/// Represents a market data tick (price update).
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    /// The trading pair for this price update.
    pub pair: TradingPair,
    /// The current price.
    pub price: rust_decimal::Decimal,
    /// The timestamp of this update.
    pub timestamp: std::time::SystemTime,
}

// Re-exporting the Transaction and Event types from the Aptos indexer processor SDK
// to make them available throughout the workspace without adding the SDK as a direct
// dependency to other crates. This helps in centralizing dependency management.
pub use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Event;
pub use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::{
    transaction::TxnData, Transaction,
};
