//! Trade sizing heuristics and slippage management.

use crate::prelude::*;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

/// Configuration for trade sizing.
#[derive(Debug, Clone)]
pub struct SizingConfig {
    /// Fraction of minimum liquidity to use as max size (e.g., 0.05 = 5%)
    pub size_fraction: f64,
    /// Maximum allowed slippage percentage
    pub slippage_cap: f64,
    /// Minimum trade size
    pub min_size: Decimal,
    /// Maximum trade size
    pub max_size: Decimal,
}

impl Default for SizingConfig {
    fn default() -> Self {
        Self {
            size_fraction: 0.05,              // 5%
            slippage_cap: 0.05,               // 5%
            min_size: Decimal::new(1, 6),     // 0.000001
            max_size: Decimal::new(10000, 0), // 10,000
        }
    }
}

/// Trade sizing calculator.
pub struct TradeSizer {
    config: SizingConfig,
}

impl TradeSizer {
    /// Creates a new trade sizer with the given configuration.
    pub fn new(config: SizingConfig) -> Self {
        Self { config }
    }

    /// Creates a new trade sizer with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SizingConfig::default())
    }

    /// Public getter for the SizingConfig's min_size.
    pub fn min_size(&self) -> Decimal {
        self.config.min_size
    }

    /// Public getter for the SizingConfig's slippage_cap.
    pub fn slippage_cap(&self) -> f64 {
        self.config.slippage_cap
    }

    /// Calculates maximum trade size for an asset based on minimum liquidity edge.
    /// max_size(asset) = min_liquidity_edge(asset) × cfg.size_fraction
    pub fn calculate_max_size(&self, asset: &Asset, snapshot: &PriceGraphSnapshot) -> Decimal {
        let min_liquidity = self.find_min_liquidity_for_asset(asset, snapshot);
        let max_size =
            min_liquidity * Decimal::from_f64(self.config.size_fraction).unwrap_or(Decimal::ZERO);

        // Clamp to configured bounds
        max_size.max(self.config.min_size).min(self.config.max_size)
    }

    /// Finds the minimum liquidity available for an asset across all its edges.
    fn find_min_liquidity_for_asset(
        &self,
        asset: &Asset,
        snapshot: &PriceGraphSnapshot,
    ) -> Decimal {
        let mut min_liquidity = Decimal::MAX;

        for (source, target, edge) in snapshot.all_edges() {
            if source == asset || target == asset {
                let liquidity = self.extract_liquidity_from_edge(edge, asset, source == asset);
                if liquidity > Decimal::ZERO && liquidity < min_liquidity {
                    min_liquidity = liquidity;
                }
            }
        }

        if min_liquidity == Decimal::MAX {
            self.config.max_size // Default if no edges found
        } else {
            min_liquidity
        }
    }

    /// Extracts the relevant liquidity from an edge for the given asset.
    fn extract_liquidity_from_edge(&self, edge: &Edge, asset: &Asset, is_source: bool) -> Decimal {
        match &edge.model {
            PoolModel::ConstantProduct {
                reserve_x,
                reserve_y,
                ..
            } => {
                if is_source {
                    // If asset is the source, we care about the input reserve
                    if edge.pair.asset_x == *asset {
                        reserve_x.0
                    } else {
                        reserve_y.0
                    }
                } else {
                    // If asset is the target, we care about the output reserve
                    if edge.pair.asset_y == *asset {
                        reserve_y.0
                    } else {
                        reserve_x.0
                    }
                }
            }
            PoolModel::ConcentratedLiquidity { ticks, .. } => {
                // For CLMM, sum up all tick liquidity as an approximation
                ticks.iter().map(|tick| tick.liquidity_gross).sum()
            }
        }
    }

    /// Binary searches for the optimal amount that stays below slippage cap.
    pub fn find_optimal_amount_for_slippage(
        &self,
        edge: &Edge,
        max_amount: Decimal,
    ) -> Option<Decimal> {
        let mut low = self.config.min_size;
        let mut high = max_amount;
        let mut best_amount = low;

        // Get the base rate (rate with minimal amount)
        let base_rate = self.calculate_rate(edge, &Quantity(low))?;

        // Binary search for largest amount within slippage cap
        for _ in 0..20 {
            // Max 20 iterations
            if high - low < Decimal::new(1, 8) {
                // Precision: 0.00000001
                break;
            }

            let mid = (low + high) / Decimal::new(2, 0);
            let current_rate = self.calculate_rate(edge, &Quantity(mid))?;

            let slippage = self.calculate_slippage(base_rate, current_rate);

            if slippage <= self.config.slippage_cap {
                best_amount = mid;
                low = mid;
            } else {
                high = mid;
            }
        }

        Some(best_amount)
    }

    /// Calculates the exchange rate for a given amount on an edge.
    pub fn calculate_rate(&self, edge: &Edge, amount_in: &Quantity) -> Option<f64> {
        let amount_out = edge.quote(amount_in)?;

        if amount_in.0.is_zero() {
            return None;
        }

        let rate = amount_out.0 / amount_in.0;
        rate.to_string().parse().ok()
    }

    /// Calculates slippage percentage between base rate and current rate.
    pub fn calculate_slippage(&self, base_rate: f64, current_rate: f64) -> f64 {
        if base_rate == 0.0 {
            return f64::INFINITY;
        }

        ((base_rate - current_rate) / base_rate).abs()
    }

    /// Generates a range of trade sizes for testing arbitrage opportunities.
    pub fn generate_trade_sizes(
        &self,
        asset: &Asset,
        snapshot: &PriceGraphSnapshot,
    ) -> Vec<Decimal> {
        let max_size = self.calculate_max_size(asset, snapshot);

        vec![
            self.config.min_size,           // Epsilon
            max_size * Decimal::new(25, 2), // 25% of max
            max_size * Decimal::new(50, 2), // 50% of max
            max_size * Decimal::new(75, 2), // 75% of max
            max_size,                       // Max size
        ]
        .into_iter()
        .filter(|&size| size >= self.config.min_size && size <= self.config.max_size)
        .collect()
    }

    /// Calculates price impact for a trade.
    pub fn calculate_price_impact(&self, edge: &Edge, amount_in: &Quantity) -> Option<f64> {
        // Get rate with minimal amount (approximates spot price)
        let spot_rate = self.calculate_rate(edge, &Quantity(Decimal::new(1, 8)))?;

        // Get rate with actual amount
        let actual_rate = self.calculate_rate(edge, amount_in)?;

        // Price impact = (spot_rate - actual_rate) / spot_rate
        if spot_rate == 0.0 {
            None
        } else {
            Some((spot_rate - actual_rate) / spot_rate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn create_test_sizer() -> TradeSizer {
        TradeSizer::with_defaults()
    }

    fn create_test_snapshot() -> PriceGraphSnapshot {
        let mut graph = PriceGraphImpl::new();

        let usdc = Asset::from_str("USDC").unwrap();
        let apt = Asset::from_str("APT").unwrap();

        let edge = Edge {
            pair: TradingPair::new(usdc.clone(), apt.clone()),
            exchange: ExchangeId::pancakeswap_v3(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(10000)),
                reserve_y: Quantity(dec!(1000)),
                fee_bps: 30,
            },
            last_updated: std::time::Instant::now(),
        };

        graph.upsert_edge(edge);
        graph.snapshot()
    }

    #[test]
    fn test_sizer_creation() {
        let sizer = create_test_sizer();
        assert!(sizer.config.size_fraction > 0.0);
        assert!(sizer.config.slippage_cap > 0.0);
    }

    #[test]
    fn test_calculate_max_size() {
        let sizer = create_test_sizer();
        let snapshot = create_test_snapshot();
        let usdc = Asset::from_str("USDC").unwrap();

        let max_size = sizer.calculate_max_size(&usdc, &snapshot);

        // Should be 5% of 10000 = 500
        let expected = dec!(10000) * Decimal::from_f64(0.05).unwrap();
        assert_eq!(max_size, expected);
    }

    #[test]
    fn test_find_min_liquidity_for_asset() {
        let sizer = create_test_sizer();
        let snapshot = create_test_snapshot();
        let usdc = Asset::from_str("USDC").unwrap();

        let min_liquidity = sizer.find_min_liquidity_for_asset(&usdc, &snapshot);

        // Should find the USDC reserve which is 10000
        assert_eq!(min_liquidity, dec!(10000));
    }

    #[test]
    fn test_calculate_rate() {
        let sizer = create_test_sizer();

        let edge = Edge {
            pair: TradingPair::new(
                Asset::from_str("USDC").unwrap(),
                Asset::from_str("APT").unwrap(),
            ),
            exchange: ExchangeId::pancakeswap_v3(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(10000)),
                reserve_y: Quantity(dec!(1000)),
                fee_bps: 30,
            },
            last_updated: std::time::Instant::now(),
        };

        let rate = sizer.calculate_rate(&edge, &Quantity(dec!(100)));

        assert!(rate.is_some());
        let rate_val = rate.unwrap();
        assert!(rate_val > 0.0);
        assert!(rate_val < 1.0); // Should be less than 1 (100 USDC -> ~9.9 APT)
    }

    #[test]
    fn test_generate_trade_sizes() {
        let sizer = create_test_sizer();
        let snapshot = create_test_snapshot();
        let usdc = Asset::from_str("USDC").unwrap();

        let sizes = sizer.generate_trade_sizes(&usdc, &snapshot);

        assert!(!sizes.is_empty());
        assert!(sizes.len() <= 5);

        // Sizes should be in ascending order
        for i in 1..sizes.len() {
            assert!(sizes[i] >= sizes[i - 1]);
        }
    }

    #[test]
    fn test_calculate_price_impact() {
        let sizer = create_test_sizer();

        let edge = Edge {
            pair: TradingPair::new(
                Asset::from_str("USDC").unwrap(),
                Asset::from_str("APT").unwrap(),
            ),
            exchange: ExchangeId::pancakeswap_v3(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(10000)),
                reserve_y: Quantity(dec!(1000)),
                fee_bps: 30,
            },
            last_updated: std::time::Instant::now(),
        };

        let impact = sizer.calculate_price_impact(&edge, &Quantity(dec!(1000)));

        assert!(impact.is_some());
        let impact_val = impact.unwrap();
        assert!(impact_val >= 0.0); // Price impact should be positive
        assert!(impact_val < 1.0); // Should be less than 100%
    }

    #[test]
    fn test_find_optimal_amount_for_slippage() {
        let sizer = create_test_sizer();

        let edge = Edge {
            pair: TradingPair::new(
                Asset::from_str("USDC").unwrap(),
                Asset::from_str("APT").unwrap(),
            ),
            exchange: ExchangeId::pancakeswap_v3(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(10000)),
                reserve_y: Quantity(dec!(1000)),
                fee_bps: 30,
            },
            last_updated: std::time::Instant::now(),
        };

        let optimal_amount = sizer.find_optimal_amount_for_slippage(&edge, dec!(1000));

        assert!(optimal_amount.is_some());
        let amount = optimal_amount.unwrap();
        assert!(amount > Decimal::ZERO);
        assert!(amount <= dec!(1000));
    }
}
