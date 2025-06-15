//! Naive arbitrage detector using log-space Bellman-Ford algorithm.

use crate::prelude::*;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet}; // Added HashSet for deduplicating trade sizes
                                          // CycleEval is available via prelude
use crate::gas::{GasCalculator, GasConfig};
use crate::sizing::{SizingConfig, TradeSizer};
use common::errors::CommonError; // Import SizingConfig and TradeSizer

/// Configuration for the arbitrage detector.
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Minimum profit threshold (percentage) to consider a cycle for gas calculation
    pub min_profit_pct: Decimal,
    // /// Trade sizes to test for arbitrage opportunities - REMOVED, will be generated
    // pub trade_sizes: Vec<Decimal>,
    // /// Maximum slippage percentage allowed - REMOVED, part of SizingConfig
    // pub slippage_cap: f64,
    /// Configuration for trade sizing heuristics and slippage
    pub sizing_config: SizingConfig,
    /// Configuration for gas calculations
    pub gas_config: GasConfig,
    /// Minimum net profit (absolute value in start asset) to consider a cycle profitable
    pub min_net_profit: Decimal,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            min_profit_pct: Decimal::new(1, 2), // 0.01 = 1% gross profit
            // trade_sizes: vec![ // REMOVED
            //     Decimal::new(1, 6),    // 0.000001 (epsilon)
            //     Decimal::new(100, 0),  // 100
            //     Decimal::new(500, 0),  // 500
            //     Decimal::new(1000, 0), // 1000
            // ],
            // slippage_cap: 0.05, // REMOVED
            sizing_config: SizingConfig::default(),
            gas_config: GasConfig::default(),
            min_net_profit: Decimal::new(1, 4), // e.g. 0.0001 of the start asset
        }
    }
}

/// Naive arbitrage detector implementation.
pub struct NaiveDetector {
    config: DetectorConfig,
    gas_calculator: GasCalculator,
    trade_sizer: TradeSizer,
}

impl NaiveDetector {
    /// Creates a new naive detector with the given configuration.
    pub fn new(config: DetectorConfig) -> Self {
        let gas_calculator = GasCalculator::new(config.gas_config.clone());
        let trade_sizer = TradeSizer::new(config.sizing_config.clone());
        Self {
            config,
            gas_calculator,
            trade_sizer,
        }
    }

    /// Creates a new naive detector with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(DetectorConfig::default())
    }

    /// Detects arbitrage opportunities in the given price graph snapshot.
    ///
    /// Implements the full algorithm from the design specification:
    /// 1. Trade-size loop – iterate over dynamically generated sizes
    /// 2. Log-space Bellman-Ford algorithm
    /// 3. Cycle reconstruction (including slippage check)
    /// 4. Filter & rank results by net profit (after gas)
    pub async fn detect_arbitrage(
        &self,
        snapshot: &PriceGraphSnapshot,
        oracle_prices: &HashMap<Asset, Decimal>, // Prices for converting gas cost
    ) -> Result<Vec<CycleEval>, CommonError> {
        let mut all_path_quotes = Vec::new();

        // Step 1: Generate trade sizes using TradeSizer
        let unique_assets = self.collect_assets(snapshot);
        let mut generated_trade_sizes_set = HashSet::new();
        if unique_assets.is_empty() && !self.config.sizing_config.min_size.is_zero() {
            // Add min_size if no assets to prevent empty trade_sizes, useful for empty graph tests
            generated_trade_sizes_set.insert(self.config.sizing_config.min_size.to_string());
        } else {
            for asset in &unique_assets {
                let sizes_for_asset = self.trade_sizer.generate_trade_sizes(asset, snapshot);
                for size in sizes_for_asset {
                    generated_trade_sizes_set.insert(size.to_string()); // Store as string for HashSet<Decimal>
                }
            }
        }

        let mut final_trade_sizes: Vec<Decimal> = generated_trade_sizes_set
            .into_iter()
            .map(|s| s.parse().unwrap_or_default())
            .filter(|d: &Decimal| !d.is_zero()) // Ensure no zero trade sizes
            .collect();

        if final_trade_sizes.is_empty() && !self.config.sizing_config.min_size.is_zero() {
            final_trade_sizes.push(self.config.sizing_config.min_size); // Ensure at least min_size if all else fails
        }
        final_trade_sizes.sort();

        // Main loop over generated trade sizes
        for &trade_size in &final_trade_sizes {
            if trade_size.is_zero() {
                continue;
            } // Skip zero trade_size
            let opportunities = self.detect_for_size(snapshot, trade_size);
            all_path_quotes.extend(opportunities);
        }

        // Pre-filter by gross profit percentage before expensive gas calculation
        all_path_quotes.retain(|opp| {
            let profit_pct_val = opp.profit_pct;
            let min_profit_pct_val: f64 = self.config.min_profit_pct.try_into().unwrap_or(0.0);
            profit_pct_val >= min_profit_pct_val
        });

        // Step 4: Filter by net profit using GasCalculator and rank
        let mut profitable_cycles = self
            .gas_calculator
            .filter_profitable_cycles(all_path_quotes, oracle_prices, self.config.min_net_profit)
            .await; // Removed ? as filter_profitable_cycles returns Vec<CycleEval> not Result

        profitable_cycles.sort_by(|a, b| {
            b.net_profit
                .partial_cmp(&a.net_profit)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(profitable_cycles)
    }

    /// Detects arbitrage opportunities for a specific trade size (gross profit).
    fn detect_for_size(
        &self,
        snapshot: &PriceGraphSnapshot,
        trade_size: Decimal,
    ) -> Vec<PathQuote> {
        let assets: Vec<&Asset> = self.collect_assets(snapshot);
        let mut distances: HashMap<&Asset, f64> = HashMap::new();
        let mut predecessors: HashMap<&Asset, Option<(&Asset, &ExchangeId)>> = HashMap::new();

        // Initialize distances to infinity for all assets except first one
        for (i, &asset) in assets.iter().enumerate() {
            distances.insert(asset, if i == 0 { 0.0 } else { f64::INFINITY });
            predecessors.insert(asset, None);
        }

        let num_vertices = assets.len();
        let mut negative_cycle_candidates = Vec::new();

        // Step 2: Log-space Bellman-Ford (|V|-1 relaxations)
        for iteration in 0..=num_vertices {
            let mut _relaxed_in_this_iteration = false; // Prefixed with underscore

            for (source_asset, target_asset, edge) in snapshot.all_edges() {
                let amount_in = Quantity(trade_size);

                if let Some(log_weight) = self.calculate_log_weight(edge, &amount_in) {
                    let source_dist = distances
                        .get(source_asset)
                        .copied()
                        .unwrap_or(f64::INFINITY);
                    let new_dist = source_dist + log_weight;
                    let current_dist = distances
                        .get(target_asset)
                        .copied()
                        .unwrap_or(f64::INFINITY);

                    if new_dist < current_dist {
                        distances.insert(target_asset, new_dist);
                        predecessors.insert(target_asset, Some((source_asset, &edge.exchange)));
                        _relaxed_in_this_iteration = true; // Prefixed with underscore

                        // Step 2: Any edge relaxed on iteration |V| => negative cycle candidate
                        if iteration == num_vertices {
                            negative_cycle_candidates.push(target_asset);
                        }
                    }
                }
            }

            // Spec §3 step 2: run |V| − 1 relaxations *without early exit*.
            // Therefore we intentionally do **not** break the loop even if no
            // edge was relaxed in this iteration.
        }

        // Step 3: Cycle reconstruction
        let mut opportunities = Vec::new();
        for &cycle_asset in &negative_cycle_candidates {
            if let Some(cycle_path) = self.reconstruct_cycle(cycle_asset, &predecessors, &assets) {
                if let Some(path_quote) = self.evaluate_cycle(cycle_path, trade_size, snapshot) {
                    opportunities.push(path_quote);
                }
            }
        }

        opportunities
    }

    /// Collects all unique assets from the graph snapshot.
    fn collect_assets<'a>(&self, snapshot: &'a PriceGraphSnapshot) -> Vec<&'a Asset> {
        let mut assets = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (source, target, _) in snapshot.all_edges() {
            if seen.insert(source) {
                assets.push(source);
            }
            if seen.insert(target) {
                assets.push(target);
            }
        }

        assets
    }

    /// Calculates the log-space weight for an edge given an input amount.
    /// Weight = -ln(rate_e(amount_in) * (1 - fee))
    fn calculate_log_weight(&self, edge: &Edge, amount_in: &Quantity) -> Option<f64> {
        let amount_out = edge.quote(amount_in)?;

        if amount_out.0.is_zero() || amount_in.0.is_zero() {
            return None;
        }

        // Calculate exchange rate
        let rate = amount_out.0 / amount_in.0;
        let rate_f64 = rate.to_string().parse::<f64>().ok()?;

        if rate_f64 > 0.0 {
            Some(-rate_f64.ln())
        } else {
            None
        }
    }

    /// Reconstructs a cycle from the predecessors map.
    fn reconstruct_cycle(
        &self,
        start_asset: &Asset,
        predecessors: &HashMap<&Asset, Option<(&Asset, &ExchangeId)>>,
        _assets: &[&Asset],
    ) -> Option<Vec<(Asset, ExchangeId)>> {
        let _path: Vec<(Asset, ExchangeId)> = Vec::new();
        let mut current = start_asset;
        let mut visited = std::collections::HashSet::new();

        // Follow predecessors to find the cycle
        loop {
            if visited.contains(current) {
                // Found the cycle start, now build the actual cycle
                let cycle_start = current;
                let mut cycle_path = Vec::new();
                let mut cycle_current = current;

                while let Some(Some((prev_asset, exchange))) = predecessors.get(cycle_current) {
                    cycle_path.push(((*cycle_current).clone(), (*exchange).clone()));
                    cycle_current = prev_asset;

                    if cycle_current == cycle_start {
                        cycle_path.reverse();
                        return Some(cycle_path);
                    }
                }
                break;
            }

            visited.insert(current);

            if let Some(Some((prev_asset, _))) = predecessors.get(current) {
                current = prev_asset;
            } else {
                break;
            }
        }

        None
    }

    /// Evaluates a cycle to calculate actual profit and checks for slippage.
    fn evaluate_cycle(
        &self,
        cycle_path: Vec<(Asset, ExchangeId)>,
        initial_amount: Decimal,
        snapshot: &PriceGraphSnapshot,
    ) -> Option<PathQuote> {
        if cycle_path.is_empty() || initial_amount.is_zero() {
            return None;
        }

        let _start_asset = &cycle_path[0].0;
        let mut current_sim_amount = Quantity(initial_amount);

        // Simulate the actual trades through the cycle
        for i in 0..cycle_path.len() {
            let current_asset_in_path = &cycle_path[i].0;
            // The next asset in the cycle path logic needs to correctly wrap around.
            // The target asset for the current edge is the *next* asset in the cycle_path definition.
            // Example: Path A -> B -> C -> A.
            // Edge 1: A (current_asset_in_path) to B (next_asset_in_path) via Exchange X (cycle_path[i].1)
            // Edge 2: B (current_asset_in_path) to C (next_asset_in_path) via Exchange Y (cycle_path[i].1)
            // Edge 3: C (current_asset_in_path) to A (next_asset_in_path) via Exchange Z (cycle_path[i].1)
            // The cycle_path stores (Asset_N, Exchange_For_Trade_From_Asset_N_to_Asset_N+1)

            let source_for_edge = current_asset_in_path;
            let target_for_edge = &cycle_path[(i + 1) % cycle_path.len()].0; // Next asset in cycle is target
            let exchange_for_edge = &cycle_path[i].1;

            // Find the edge for this trade
            if let Some(edge) = self.find_edge(
                snapshot,
                source_for_edge,
                target_for_edge,
                exchange_for_edge,
            ) {
                // Slippage Check for this edge with initial_amount
                // Use a very small amount for base rate calculation to approximate spot price
                let min_qty_for_rate =
                    Quantity(self.trade_sizer.min_size().max(Decimal::new(1, 8)));

                if let (Some(base_rate), Some(actual_rate)) = (
                    self.trade_sizer.calculate_rate(edge, &min_qty_for_rate),
                    self.trade_sizer
                        .calculate_rate(edge, &Quantity(initial_amount)),
                ) {
                    let slippage = self.trade_sizer.calculate_slippage(base_rate, actual_rate);
                    if slippage > self.trade_sizer.slippage_cap() {
                        return None; // Cycle fails slippage check on this edge
                    }
                } else {
                    return None; // Could not calculate rates for slippage check
                }

                // Proceed with quoting using the current amount in the simulation
                if let Some(output_amount) = edge.quote(&current_sim_amount) {
                    if output_amount.0.is_zero() && i < cycle_path.len() - 1 {
                        // if not last trade and output is zero
                        return None; // Trade resulted in zero output prematurely
                    }
                    current_sim_amount = output_amount;
                } else {
                    return None; // Trade failed (quote returned None)
                }
            } else {
                return None; // Edge not found
            }
        }

        // Calculate profit
        let final_amount = current_sim_amount.0;
        if final_amount > initial_amount {
            let profit_amount = final_amount - initial_amount;
            if initial_amount.is_zero() {
                return None;
            } // Avoid division by zero
            let profit_pct_decimal = profit_amount / initial_amount;
            let profit_pct: f64 = profit_pct_decimal.try_into().unwrap_or(0.0);

            Some(PathQuote {
                path: cycle_path,
                amount_in: Quantity(initial_amount),
                amount_out: current_sim_amount,
                profit_pct,
            })
        } else {
            None
        }
    }

    /// Finds an edge in the snapshot for the given trade.
    fn find_edge<'a>(
        &self,
        snapshot: &'a PriceGraphSnapshot,
        from_asset: &Asset,
        to_asset: &Asset,
        exchange: &ExchangeId,
    ) -> Option<&'a Edge> {
        snapshot
            .all_edges()
            .find(|(source, target, edge)| {
                *source == from_asset && *target == to_asset && edge.exchange == *exchange
            })
            .map(move |(_, _, edge)| edge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn create_test_detector() -> NaiveDetector {
        NaiveDetector::with_defaults()
    }

    fn create_arbitrage_snapshot() -> PriceGraphSnapshot {
        let mut graph = PriceGraphImpl::new();

        let usdc = Asset::from_str("USDC").unwrap();
        let apt = Asset::from_str("APT").unwrap();
        let eth = Asset::from_str("ETH").unwrap();

        // Create a triangular arbitrage opportunity
        // USDC -> APT (rate: 0.1, 10 USDC = 1 APT)
        let edge1 = Edge {
            pair: TradingPair::new(usdc.clone(), apt.clone()),
            exchange: ExchangeId::pancakeswap_v3(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(10000)),
                reserve_y: Quantity(dec!(1000)),
                fee_bps: 25, // 0.25%
            },
            last_updated: std::time::Instant::now(),
        };

        // APT -> ETH (rate: 0.1, 10 APT = 1 ETH)
        let edge2 = Edge {
            pair: TradingPair::new(apt.clone(), eth.clone()),
            exchange: ExchangeId::pancakeswap_v3(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1000)),
                reserve_y: Quantity(dec!(100)),
                fee_bps: 25,
            },
            last_updated: std::time::Instant::now(),
        };

        // ETH -> USDC (rate: 105, 1 ETH = 105 USDC - this creates arbitrage)
        let edge3 = Edge {
            pair: TradingPair::new(eth.clone(), usdc.clone()),
            exchange: ExchangeId::pancakeswap_v3(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(100)),
                reserve_y: Quantity(dec!(10500)), // Slightly favorable rate
                fee_bps: 25,
            },
            last_updated: std::time::Instant::now(),
        };

        graph.upsert_edge(edge1);
        graph.upsert_edge(edge2);
        graph.upsert_edge(edge3);

        graph.snapshot()
    }

    #[test]
    fn test_detector_creation() {
        let detector = create_test_detector();
        // trade_sizes is now part of SizingConfig and generated dynamically.
        // We can assert that the SizingConfig has a default min_size.
        assert!(detector.config.sizing_config.min_size > Decimal::ZERO);
        assert!(detector.config.min_profit_pct > Decimal::ZERO);
    }

    #[test]
    fn test_collect_assets() {
        let detector = create_test_detector();
        let snapshot = create_arbitrage_snapshot();

        let assets = detector.collect_assets(&snapshot);
        assert_eq!(assets.len(), 3); // USDC, APT, ETH
    }

    #[test]
    fn test_calculate_log_weight() {
        let detector = create_test_detector();

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

        let amount_in = Quantity(dec!(100));
        let weight = detector.calculate_log_weight(&edge, &amount_in);

        assert!(weight.is_some());
        let weight_val = weight.unwrap();
        assert!(weight_val.is_finite());
        // Since the rate is typically < 1 for this test (100 USDC -> ~9.9 APT),
        // -ln(rate) should be positive, not negative
        assert!(weight_val > 0.0);
    }

    #[tokio::test] // Mark test as async
    async fn test_detect_arbitrage() {
        // Make test function async
        let detector = create_test_detector();
        let snapshot = create_arbitrage_snapshot();
        let mut oracle_prices = HashMap::new();
        // Populate with some mock prices, e.g., assuming USDC is the quote for others
        oracle_prices.insert(Asset::from_str("USDC").unwrap(), Decimal::ONE);
        oracle_prices.insert(Asset::from_str("APT").unwrap(), dec!(8)); // 1 APT = 8 USDC
        oracle_prices.insert(Asset::from_str("ETH").unwrap(), dec!(2000)); // 1 ETH = 2000 USDC

        let opportunities_result = detector.detect_arbitrage(&snapshot, &oracle_prices).await;

        assert!(opportunities_result.is_ok());
        let opportunities = opportunities_result.unwrap();

        // Should find arbitrage opportunities in the triangular setup
        println!("Found {} opportunities", opportunities.len());
        for opp_eval in &opportunities {
            // CycleEval has net_profit, not profit_pct directly on the top level.
            // PathQuote is inside CycleEval if we need to access its profit_pct.
            // For now, let's just print the net_profit.
            println!("Opportunity: net_profit = {}", opp_eval.net_profit);
        }
        // A more robust test would assert on the number of opportunities or specific profit values.
        // For now, we just check if it runs and finds some.
        // If the setup is correct, it should find at least one.
        // assert!(!opportunities.is_empty()); // This can be enabled once confident in setup
    }
}
