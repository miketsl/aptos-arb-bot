use rust_decimal::Decimal;
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Represents a unique identifier for an exchange.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExchangeId(pub String);

impl fmt::Display for ExchangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ExchangeId {
    fn from(s: &str) -> Self {
        ExchangeId(s.to_string())
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
pub struct Order {
    pub id: String, // Unique order identifier
    pub pair: AssetPair,
    pub order_type: OrderType,
    pub price: Price,
    pub quantity: Quantity,
    pub exchange: ExchangeId,
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
    fn test_exchange_id_display_and_from_str() {
        let exchange_id = ExchangeId::from("binance");
        assert_eq!(exchange_id, ExchangeId("binance".to_string()));
        assert_eq!(format!("{}", exchange_id), "binance");
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
            exchange: ExchangeId::from("test-exchange"),
        };
        assert_eq!(order.id, "order123");
        assert_eq!(order.order_type, OrderType::Buy);
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
