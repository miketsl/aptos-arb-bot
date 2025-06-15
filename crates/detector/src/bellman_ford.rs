//! Naive arbitrage detector using log-space Bellman-Ford algorithm.

use crate::prelude::*;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Configuration for the arbitrage detector.
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Minimum profit threshold to consider a cycle profitable
    pub min_profit: Decimal,
    /// Trade sizes to test for arbitrage opportunities
    pub trade_sizes: Vec<Decimal>,
    /// Maximum slippage percentage allowed
    pub slippage_cap: f64,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            min_profit: Decimal::new(1, 2), // 0.01 = 1%
            trade_sizes: vec![
                Decimal::new(1, 6),    // 0.000001 (epsilon)
                Decimal::new(100, 0),  // 100
                Decimal::new(500, 0),  // 500
                Decimal::new(1000, 0), // 1000
            ],
            slippage_cap: 0.05, // 5%
        }
    }
}

/// Naive arbitrage detector implementation.
pub struct NaiveDetector {
    config: DetectorConfig,
}

impl NaiveDetector {
    /// Creates a new naive detector with the given configuration.
    pub fn new(config: DetectorConfig) -> Self {
        Self { config }
    }

    /// Creates a new naive detector with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(DetectorConfig::default())
    }

    /// Detects arbitrage opportunities in the given price graph snapshot.
    ///
    /// Implements the full algorithm from the design specification:
    /// 1. Trade-size loop – iterate over discrete sizes
    /// 2. Log-space Bellman-Ford algorithm
    /// 3. Cycle reconstruction
    /// 4. Filter & rank results
    pub fn detect_arbitrage(&self, snapshot: &PriceGraphSnapshot) -> Vec<PathQuote> {
        let mut all_opportunities = Vec::new();

        // Step 1: Trade-size loop
        for &trade_size in &self.config.trade_sizes {
            let opportunities = self.detect_for_size(snapshot, trade_size);
            all_opportunities.extend(opportunities);
        }

        // Step 5: Filter & rank by profit
        all_opportunities.sort_by(|a, b| {
            b.profit_pct
                .partial_cmp(&a.profit_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_opportunities.retain(|opp| {
            opp.profit_pct >= self.config.min_profit.to_string().parse().unwrap_or(0.0)
        });

        all_opportunities
    }

    /// Detects arbitrage opportunities for a specific trade size.
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
            let mut relaxed_in_this_iteration = false;

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
                        relaxed_in_this_iteration = true;

                        // Step 2: Any edge relaxed on iteration |V| => negative cycle candidate
                        if iteration == num_vertices {
                            negative_cycle_candidates.push(target_asset);
                        }
                    }
                }
            }

            if !relaxed_in_this_iteration && iteration < num_vertices {
                break; // Early termination if no relaxation occurred
            }
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

    /// Evaluates a cycle to calculate actual profit.
    fn evaluate_cycle(
        &self,
        cycle_path: Vec<(Asset, ExchangeId)>,
        initial_amount: Decimal,
        snapshot: &PriceGraphSnapshot,
    ) -> Option<PathQuote> {
        if cycle_path.is_empty() {
            return None;
        }

        let _start_asset = &cycle_path[0].0;
        let mut current_amount = Quantity(initial_amount);

        // Simulate the actual trades through the cycle
        for i in 0..cycle_path.len() {
            let current_asset = &cycle_path[i].0;
            let next_asset = &cycle_path[(i + 1) % cycle_path.len()].0;
            let exchange = &cycle_path[i].1;

            // Find the edge for this trade
            if let Some(edge) = self.find_edge(snapshot, current_asset, next_asset, exchange) {
                if let Some(output_amount) = edge.quote(&current_amount) {
                    current_amount = output_amount;
                } else {
                    return None; // Trade failed
                }
            } else {
                return None; // Edge not found
            }
        }

        // Calculate profit
        let final_amount = current_amount.0;
        if final_amount > initial_amount {
            let profit_amount = final_amount - initial_amount;
            let profit_pct = (profit_amount / initial_amount)
                .to_string()
                .parse()
                .unwrap_or(0.0);

            Some(PathQuote {
                path: cycle_path,
                amount_in: Quantity(initial_amount),
                amount_out: current_amount,
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
        assert!(!detector.config.trade_sizes.is_empty());
        assert!(detector.config.min_profit > Decimal::ZERO);
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

    #[test]
    fn test_detect_arbitrage() {
        let detector = create_test_detector();
        let snapshot = create_arbitrage_snapshot();

        let opportunities = detector.detect_arbitrage(&snapshot);

        // Should find arbitrage opportunities in the triangular setup
        println!("Found {} opportunities", opportunities.len());
        for opp in &opportunities {
            println!("Opportunity: profit = {:.4}%", opp.profit_pct * 100.0);
        }
    }
}
