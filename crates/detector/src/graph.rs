use crate::prelude::*;
use common::types::{Asset, Quantity, TradingPair};
use dex_adapter_trait::Exchange;
use petgraph::graphmap::DiGraphMap;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A copyable asset identifier for use as DiGraphMap node
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(u64);

impl AssetId {
    fn new(id: u64) -> Self {
        AssetId(id)
    }
}

/// Represents the model of a liquidity pool.
#[derive(Debug, Clone, PartialEq)]
pub enum PoolModel {
    ConstantProduct {
        reserve_x: Quantity,
        reserve_y: Quantity,
        fee_bps: u16,
    },
    ConcentratedLiquidity {
        ticks: Vec<Tick>,
        fee_bps: u16,
    },
}

/// Represents a tick in a concentrated liquidity pool.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub price: Decimal,
    pub liquidity_gross: Decimal,
}

/// Represents an edge in the price graph, corresponding to a specific swap direction in a pool.
/// The Edge itself will be the weight in the DiGraphMap.
/// To ensure PartialEq and Eq for DiGraphMap, we might need to be careful if Edge contains non-Eq fields like Instant directly.
/// However, petgraph::graphmap::DiGraphMap uses the node type's Eq and Hash. Edge is a weight.
#[derive(Debug, Clone)]
pub struct Edge {
    pub pair: TradingPair, // Defines direction: asset_x -> asset_y
    pub exchange: Exchange,
    pub model: PoolModel,
    pub last_updated: Instant,
}

// Manual PartialEq implementation for Edge if needed due to Instant, though for graph weights it's not strictly necessary for map keys.
// For now, relying on derive(Clone) and using it as a weight.
// If Edge were a node, it would need Eq and Hash.
impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        // Equality should be deterministic and must **not** depend on wall-clock
        // time. `last_updated` is therefore intentionally *excluded*.
        self.pair == other.pair && self.exchange == other.exchange && self.model == other.model
    }
}

impl Edge {
    /// Calculates the output amount for a given input amount based on the pool model.
    pub fn quote(&self, amount_in: &Quantity, asset_in: &Asset) -> Option<Quantity> {
        if *asset_in != self.pair.asset_x {
            return None;
        }
        match &self.model {
            PoolModel::ConstantProduct {
                reserve_x,
                reserve_y,
                fee_bps,
            } => {
                // For tuple struct Quantity, we can't check asset directly
                // The caller must ensure the asset matches

                if reserve_x.0.is_zero() || reserve_y.0.is_zero() || amount_in.0.is_zero() {
                    return None;
                }

                let amount_in_val = amount_in.0;
                let reserve_x_val = reserve_x.0;
                let reserve_y_val = reserve_y.0;

                let fee_decimal =
                    Decimal::from_u16(*fee_bps).unwrap_or_default() / Decimal::new(10000, 0);
                let amount_in_after_fee = amount_in_val * (Decimal::ONE - fee_decimal);

                // Classic CPMM: (x + dx_eff) * (y - dy) = x * y
                // dy = y * dx_eff / (x + dx_eff)
                let amount_out_val =
                    (reserve_y_val * amount_in_after_fee) / (reserve_x_val + amount_in_after_fee);

                if amount_out_val.is_sign_negative()
                    || amount_out_val.is_zero()
                    || amount_out_val > reserve_y_val
                {
                    return None; // Not enough liquidity or invalid amount
                }
                Some(Quantity(amount_out_val))
            }
            PoolModel::ConcentratedLiquidity { ticks, fee_bps } => {
                // Simplified CLMM Quoting Logic (Order-Book Style)
                // Assumptions:
                // 1. `ticks` are discrete price levels.
                // 2. `Tick::price` is `asset_y / asset_x` (output per input).
                // 3. `Tick::liquidity_gross` is the amount of `asset_x` available at that price.
                // 4. Ticks are sorted to consume best prices first.
                // 5. Fees are applied on input.

                if amount_in.0.is_zero() {
                    return None;
                }

                let fee_decimal =
                    Decimal::from_u16(*fee_bps).unwrap_or_default() / Decimal::new(10000, 0);
                let amount_in_after_fee = amount_in.0 * (Decimal::ONE - fee_decimal);

                if amount_in_after_fee <= Decimal::ZERO {
                    return None;
                }

                let mut remaining_to_swap = amount_in_after_fee;
                let mut total_output_asset_y = Decimal::ZERO;

                // Sort ticks by price descending to get the best rate first
                // (more asset_y per asset_x)
                let mut sorted_ticks = ticks.clone();
                sorted_ticks.sort_by(|a, b| {
                    b.price
                        .partial_cmp(&a.price)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                for tick in sorted_ticks {
                    if remaining_to_swap <= Decimal::ZERO {
                        break;
                    }
                    // Ensure price and liquidity are valid
                    if tick.price <= Decimal::ZERO || tick.liquidity_gross <= Decimal::ZERO {
                        continue;
                    }

                    // Amount of asset_x that can be swapped at this tick's price
                    let swappable_asset_x_at_tick = remaining_to_swap.min(tick.liquidity_gross);

                    // Calculate asset_y output from this tick
                    total_output_asset_y += swappable_asset_x_at_tick * tick.price;
                    remaining_to_swap -= swappable_asset_x_at_tick;
                }

                if total_output_asset_y > Decimal::ZERO {
                    Some(Quantity(total_output_asset_y))
                } else {
                    // No liquidity available or input amount too small
                    None
                }
            }
        }
    }
}

/// A snapshot of the price graph, which is an immutable copy.
#[derive(Clone, Debug)]
pub struct PriceGraphSnapshot {
    graph: DiGraphMap<AssetId, Edge>,
    asset_mapping: HashMap<AssetId, Asset>,
    #[allow(dead_code)]
    reverse_mapping: HashMap<Asset, AssetId>,
}

impl PriceGraphSnapshot {
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn all_edges(&self) -> impl Iterator<Item = (&Asset, &Asset, &Edge)> + '_ {
        self.graph.all_edges().map(|(source_id, target_id, edge)| {
            let source_asset = &self.asset_mapping[&source_id];
            let target_asset = &self.asset_mapping[&target_id];
            (source_asset, target_asset, edge)
        })
    }
}

/// Defines the interface for a price graph.
///
/// The price graph is responsible for managing pools and their corresponding
/// trading edges. A key invariant is that for every liquidity pool added, two
/// directed edges are created: a forward edge (asset_x -> asset_y) and a
/// reverse edge (asset_y -> asset_x). The `upsert_pool` function handles this
/// logic automatically.
pub trait PriceGraph {
    /// Inserts or updates a pool in the graph, creating both forward and reverse edges.
    fn upsert_pool(&mut self, edge: Edge);

    /// Ingests a batch of pools.
    fn ingest_batch(&mut self, edges: Vec<Edge>);

    /// Removes edges that haven't been updated within the given `ttl`.
    fn prune_stale(&mut self, ttl: Duration);

    /// Returns an iterator over the neighbors of a given asset and the edges leading to them.
    /// Neighbors are assets reachable directly from the given `asset`.
    fn neighbors<'a>(
        &'a self,
        asset: &Asset,
    ) -> Box<dyn Iterator<Item = (&'a Asset, &'a Edge)> + 'a>;

    /// Returns an immutable snapshot of the current graph state.
    fn snapshot(&self) -> PriceGraphSnapshot;
}

/// Implementation of the `PriceGraph` trait using `petgraph::graphmap::DiGraphMap`.
#[derive(Clone, Debug)]
pub struct PriceGraphImpl {
    graph: DiGraphMap<AssetId, Edge>,
    asset_mapping: HashMap<AssetId, Asset>,
    reverse_mapping: HashMap<Asset, AssetId>,
    next_id: u64,
}

impl PriceGraphImpl {
    pub fn new() -> Self {
        PriceGraphImpl {
            graph: DiGraphMap::new(),
            asset_mapping: HashMap::new(),
            reverse_mapping: HashMap::new(),
            next_id: 0,
        }
    }

    fn get_or_create_asset_id(&mut self, asset: &Asset) -> AssetId {
        if let Some(&asset_id) = self.reverse_mapping.get(asset) {
            asset_id
        } else {
            let asset_id = AssetId::new(self.next_id);
            self.next_id += 1;
            self.asset_mapping.insert(asset_id, asset.clone());
            self.reverse_mapping.insert(asset.clone(), asset_id);
            asset_id
        }
    }
}

impl Default for PriceGraphImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl PriceGraph for PriceGraphImpl {
    fn upsert_pool(&mut self, edge: Edge) {
        // ------------------------------------------------------------------
        // Forward edge
        // ------------------------------------------------------------------
        let source_id = self.get_or_create_asset_id(&edge.pair.asset_x);
        let target_id = self.get_or_create_asset_id(&edge.pair.asset_y);
        self.graph.add_edge(source_id, target_id, edge.clone());

        // ------------------------------------------------------------------
        // Reverse edge – automatically inserted
        // ------------------------------------------------------------------
        let mut reverse_edge = edge.clone();

        // 1️⃣  Swap trading-pair direction
        reverse_edge.pair = TradingPair {
            asset_x: edge.pair.asset_y.clone(),
            asset_y: edge.pair.asset_x.clone(),
        };

        // 2️⃣  Transform pool model so that quoting logic stays correct
        reverse_edge.model = match &edge.model {
            PoolModel::ConstantProduct {
                reserve_x,
                reserve_y,
                fee_bps,
            } => PoolModel::ConstantProduct {
                reserve_x: *reserve_y,
                reserve_y: *reserve_x,
                fee_bps: *fee_bps,
            },
            PoolModel::ConcentratedLiquidity { ticks, fee_bps } => {
                // Invert every tick price (y/x  ->  x/y). Liquidity stays unchanged
                let inverted_ticks = ticks
                    .iter()
                    .map(|t| Tick {
                        price: if t.price.is_zero() {
                            Decimal::ZERO
                        } else {
                            Decimal::ONE / t.price
                        },
                        liquidity_gross: t.liquidity_gross,
                    })
                    .collect();
                PoolModel::ConcentratedLiquidity {
                    ticks: inverted_ticks,
                    fee_bps: *fee_bps,
                }
            }
        };

        let rev_src_id = self.get_or_create_asset_id(&reverse_edge.pair.asset_x);
        let rev_tgt_id = self.get_or_create_asset_id(&reverse_edge.pair.asset_y);
        self.graph.add_edge(rev_src_id, rev_tgt_id, reverse_edge);
    }

    fn ingest_batch(&mut self, edges: Vec<Edge>) {
        for edge in edges {
            self.upsert_pool(edge);
        }
    }

    fn prune_stale(&mut self, ttl: Duration) {
        let now = Instant::now();
        let mut edges_to_remove = Vec::new();

        for (source_id, target_id, edge_data) in self.graph.all_edges() {
            if now.duration_since(edge_data.last_updated) > ttl {
                edges_to_remove.push((source_id, target_id));
            }
        }

        for (source_id, target_id) in edges_to_remove {
            self.graph.remove_edge(source_id, target_id);
        }
    }

    fn neighbors<'a>(
        &'a self,
        asset: &Asset,
    ) -> Box<dyn Iterator<Item = (&'a Asset, &'a Edge)> + 'a> {
        if let Some(&asset_id) = self.reverse_mapping.get(asset) {
            Box::new(
                self.graph
                    .edges(asset_id)
                    .map(|(_, target_id, edge_data)| (&self.asset_mapping[&target_id], edge_data)),
            )
        } else {
            Box::new(std::iter::empty())
        }
    }

    fn snapshot(&self) -> PriceGraphSnapshot {
        PriceGraphSnapshot {
            graph: self.graph.clone(),
            asset_mapping: self.asset_mapping.clone(),
            reverse_mapping: self.reverse_mapping.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{Asset, Quantity, TradingPair};
    use dex_adapter_trait::Exchange;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn usdc_asset() -> Asset {
        Asset::from_str("0x1::coin::USDC").unwrap()
    }
    fn apt_asset() -> Asset {
        Asset::from_str("0x1::aptos_coin::AptosCoin").unwrap()
    }
    fn eth_asset() -> Asset {
        Asset::from_str("0x1::coin::ETH").unwrap()
    }

    fn create_test_edge(
        asset_x: Asset,
        asset_y: Asset,
        reserve_x_val: Decimal,
        reserve_y_val: Decimal,
        fee_bps: u16,
    ) -> Edge {
        Edge {
            pair: TradingPair {
                asset_x: asset_x.clone(),
                asset_y: asset_y.clone(),
            },
            exchange: Exchange::Tapp, // Placeholder
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(reserve_x_val),
                reserve_y: Quantity(reserve_y_val),
                fee_bps,
            },
            last_updated: Instant::now(),
        }
    }

    #[test]
    fn test_upsert_and_snapshot() {
        let mut graph = PriceGraphImpl::new();
        let edge1 = create_test_edge(usdc_asset(), apt_asset(), dec!(1000), dec!(100), 30);

        graph.upsert_pool(edge1.clone());

        let snapshot = graph.snapshot();
        // Now expects 2 edges due to automatic reverse edge creation
        assert_eq!(snapshot.edge_count(), 2);
        assert_eq!(snapshot.node_count(), 2);

        let mut found_forward = false;
        let mut found_reverse = false;
        for (_, _, retrieved_edge) in snapshot.all_edges() {
            if retrieved_edge.pair == edge1.pair {
                found_forward = true;
            } else if retrieved_edge.pair.asset_x == edge1.pair.asset_y
                && retrieved_edge.pair.asset_y == edge1.pair.asset_x
            {
                found_reverse = true;
                // Check if reserves are swapped for CPMM
                if let PoolModel::ConstantProduct {
                    reserve_x,
                    reserve_y,
                    ..
                } = &retrieved_edge.model
                {
                    if let PoolModel::ConstantProduct {
                        reserve_x: orig_rx,
                        reserve_y: orig_ry,
                        ..
                    } = &edge1.model
                    {
                        assert_eq!(reserve_x.0, orig_ry.0);
                        assert_eq!(reserve_y.0, orig_rx.0);
                    } else {
                        panic!("Original edge not CPMM");
                    }
                } // Add similar check for CL if necessary
            }
        }
        assert!(found_forward, "Forward edge not found");
        assert!(
            found_reverse,
            "Reverse edge not found or model not correctly inverted"
        );
    }

    #[test]
    fn test_ingest_batch() {
        let mut graph = PriceGraphImpl::new();
        let edge1 = create_test_edge(usdc_asset(), apt_asset(), dec!(1000), dec!(100), 30);
        let edge2 = create_test_edge(apt_asset(), eth_asset(), dec!(50), dec!(1), 30);

        // Each upsert creates a forward and reverse edge.
        graph.ingest_batch(vec![edge1.clone(), edge2.clone()]);

        let snapshot = graph.snapshot();
        assert_eq!(snapshot.edge_count(), 4); // 2 original + 2 reverse
        assert_eq!(snapshot.node_count(), 3);
    }

    #[test]
    fn test_prune_stale() {
        let mut graph = PriceGraphImpl::new();
        let fresh_edge_orig =
            create_test_edge(usdc_asset(), apt_asset(), dec!(1000), dec!(100), 30);
        let mut stale_edge_orig = create_test_edge(apt_asset(), eth_asset(), dec!(50), dec!(1), 30);

        stale_edge_orig.last_updated = Instant::now() - Duration::from_secs(10);
        // Also make its reverse counterpart (which will be created by upsert_pool) effectively stale
        // by setting the original's last_updated before insertion.
        // The reverse edge will clone this stale `last_updated` time.

        graph.upsert_pool(fresh_edge_orig.clone()); // Adds fresh_edge + its reverse (fresh)
        graph.upsert_pool(stale_edge_orig.clone()); // Adds stale_edge + its reverse (stale)

        // Initial state: 2 fresh (orig + rev), 2 stale (orig + rev) = 4 edges
        assert_eq!(graph.snapshot().edge_count(), 4);

        graph.prune_stale(Duration::from_secs(5));
        let snapshot_after_prune = graph.snapshot();

        // Expected: stale_edge_orig and its reverse are pruned. fresh_edge_orig and its reverse remain.
        assert_eq!(
            snapshot_after_prune.edge_count(),
            2,
            "Pruning did not leave the correct number of edges."
        );

        // Ensure the fresh forward edge remains
        let mut found_fresh_forward = false;
        // Ensure the stale forward edge is gone
        let mut found_stale_forward = false;

        for (_, _, remaining_edge) in snapshot_after_prune.all_edges() {
            if remaining_edge.pair == fresh_edge_orig.pair {
                found_fresh_forward = true;
            }
            if remaining_edge.pair == stale_edge_orig.pair {
                found_stale_forward = true;
            }
        }
        assert!(found_fresh_forward, "Fresh forward edge was pruned");
        assert!(!found_stale_forward, "Stale forward edge was not pruned");
    }

    #[test]
    fn test_neighbors() {
        let mut graph = PriceGraphImpl::new();
        let usdc = usdc_asset();
        let apt = apt_asset();
        let eth = eth_asset();

        let edge_usdc_apt = create_test_edge(usdc.clone(), apt.clone(), dec!(1000), dec!(100), 30);
        let edge_apt_eth = create_test_edge(apt.clone(), eth.clone(), dec!(50), dec!(0.5), 30);

        graph.upsert_pool(edge_usdc_apt.clone()); // Adds USDC->APT and APT->USDC
        graph.upsert_pool(edge_apt_eth.clone()); // Adds APT->ETH and ETH->APT

        // Neighbors of USDC: should be APT (from USDC->APT)
        let usdc_neighbors: Vec<_> = graph.neighbors(&usdc).collect();
        assert_eq!(usdc_neighbors.len(), 1, "USDC should have 1 neighbor (APT)");
        assert_eq!(usdc_neighbors[0].0, &apt); // Target asset
        assert_eq!(usdc_neighbors[0].1.pair.asset_x, usdc); // Edge source
        assert_eq!(usdc_neighbors[0].1.pair.asset_y, apt); // Edge target

        // Neighbors of APT: should be USDC (from APT->USDC reverse) and ETH (from APT->ETH forward)
        let apt_neighbors: Vec<_> = graph.neighbors(&apt).collect();
        assert_eq!(
            apt_neighbors.len(),
            2,
            "APT should have 2 neighbors (USDC, ETH)"
        );

        let has_usdc_as_neighbor_of_apt = apt_neighbors.iter().any(
            |(neighbor_asset, edge_from_apt)| {
                *neighbor_asset == &usdc && // Neighbor is USDC
                edge_from_apt.pair.asset_x == apt && // Edge starts from APT
                edge_from_apt.pair.asset_y == usdc
            }, // Edge goes to USDC
        );
        assert!(
            has_usdc_as_neighbor_of_apt,
            "APT should have USDC as a neighbor (via reverse of usdc_apt)"
        );

        let has_eth_as_neighbor_of_apt = apt_neighbors.iter().any(
            |(neighbor_asset, edge_from_apt)| {
                *neighbor_asset == &eth && // Neighbor is ETH
                edge_from_apt.pair.asset_x == apt && // Edge starts from APT
                edge_from_apt.pair.asset_y == eth
            }, // Edge goes to ETH
        );
        assert!(
            has_eth_as_neighbor_of_apt,
            "APT should have ETH as a neighbor (via apt_eth forward)"
        );

        // Neighbors of ETH: should be APT (from ETH->APT reverse)
        let eth_neighbors: Vec<_> = graph.neighbors(&eth).collect();
        assert_eq!(eth_neighbors.len(), 1, "ETH should have 1 neighbor (APT)");
        assert_eq!(eth_neighbors[0].0, &apt); // Target asset
        assert_eq!(eth_neighbors[0].1.pair.asset_x, eth); // Edge source
        assert_eq!(eth_neighbors[0].1.pair.asset_y, apt); // Edge target
    }

    #[test]
    fn test_edge_quote_simple_cpmm() {
        // USDC -> APT pool
        let edge = create_test_edge(usdc_asset(), apt_asset(), dec!(10000), dec!(1000), 25); // 0.25% fee

        // Quote selling 100 USDC for APT
        let amount_in_usdc = Quantity(dec!(100));
        let quote_result = edge.quote(&amount_in_usdc, &usdc_asset());

        assert!(quote_result.is_some());
        let amount_out_apt = quote_result.unwrap();

        // Manual calculation:
        // fee = 100 * (25/10000) = 0.25 USDC
        // amount_in_after_fee = 100 - 0.25 = 99.75 USDC
        // reserve_x = 10000, reserve_y = 1000
        // amount_out = reserve_y * amount_in_after_fee / (reserve_x + amount_in_after_fee)
        // amount_out = 1000 * 99.75 / (10000 + 99.75)
        // amount_out = 99750 / 10099.75 = 9.876479...
        let expected_out = dec!(1000) * (dec!(100) * (Decimal::ONE - dec!(0.0025)))
            / (dec!(10000) + (dec!(100) * (Decimal::ONE - dec!(0.0025))));
        assert_eq!(amount_out_apt.0.round_dp(8), expected_out.round_dp(8));

        // Quote selling asset not in pair (APT for APT)
        let amount_in_apt_wrong = Quantity(dec!(10));
        assert!(
            edge.quote(&amount_in_apt_wrong, &apt_asset()).is_none(),
            "Quoting with wrong asset should fail"
        );

        // Quote selling asset that is asset_y (APT for USDC, but edge is USDC -> APT)
        let amount_in_apt_reverse = Quantity(dec!(10));
        assert!(
            edge.quote(&amount_in_apt_reverse, &apt_asset()).is_none(),
            "Quoting with reverse asset should fail"
        );
    }

    #[test]
    fn test_edge_quote_insufficient_liquidity() {
        let edge = create_test_edge(usdc_asset(), apt_asset(), dec!(100), dec!(10), 25);

        // Try to swap more than available in reserve_x after fee
        let large_amount_in = Quantity(dec!(10000));
        let quote_result = edge.quote(&large_amount_in, &usdc_asset());
        // The formula itself might not show this as an error directly unless output is > reserve_y
        // Let's test if output is greater than reserve_y
        // amount_in_after_fee = 10000 * (1 - 0.0025) = 9975
        // amount_out = 10 * 9975 / (100 + 9975) = 99750 / 10075 = 9.9007...
        // This is valid and < 10.
        assert!(quote_result.is_some());
        assert!(quote_result.unwrap().0 <= dec!(10));

        // Test with zero reserve
        let zero_reserve_edge = create_test_edge(usdc_asset(), apt_asset(), dec!(0), dec!(10), 25);
        let amount_in = Quantity(dec!(10));
        assert!(zero_reserve_edge.quote(&amount_in, &usdc_asset()).is_none());

        let zero_reserve_edge_y =
            create_test_edge(usdc_asset(), apt_asset(), dec!(10), dec!(0), 25);
        assert!(zero_reserve_edge_y
            .quote(&amount_in, &usdc_asset())
            .is_none());
    }

    #[test]
    fn test_upsert_concentrated_liquidity_edge_and_reverse() {
        let mut graph = PriceGraphImpl::new();
        let asset_a = Asset::from_str("ASSET_A").unwrap();
        let asset_b = Asset::from_str("ASSET_B").unwrap();

        let forward_ticks = vec![
            Tick {
                price: dec!(100),
                liquidity_gross: dec!(10),
            }, // 1 A = 100 B
            Tick {
                price: dec!(90),
                liquidity_gross: dec!(5),
            }, // 1 A = 90 B
        ];

        let cl_edge = Edge {
            pair: TradingPair::new(asset_a.clone(), asset_b.clone()),
            exchange: Exchange::Tapp, // Placeholder
            model: PoolModel::ConcentratedLiquidity {
                ticks: forward_ticks.clone(),
                fee_bps: 10,
            },
            last_updated: Instant::now(),
        };

        graph.upsert_pool(cl_edge.clone());
        let snapshot = graph.snapshot();
        assert_eq!(snapshot.edge_count(), 2);

        let mut found_forward_cl = false;
        let mut found_reverse_cl = false;

        for (s, t, edge) in snapshot.all_edges() {
            if s == &asset_a && t == &asset_b {
                found_forward_cl = true;
                if let PoolModel::ConcentratedLiquidity { ticks, .. } = &edge.model {
                    assert_eq!(ticks.len(), 2);
                    // Assuming order is preserved from input for forward ticks
                    assert_eq!(ticks[0].price, dec!(100));
                    assert_eq!(ticks[1].price, dec!(90));
                } else {
                    panic!("Forward edge not CL");
                }
            } else if s == &asset_b && t == &asset_a {
                found_reverse_cl = true;
                if let PoolModel::ConcentratedLiquidity { ticks, .. } = &edge.model {
                    assert_eq!(
                        ticks.len(),
                        2,
                        "Reverse CL edge should have same number of ticks"
                    );
                    // Prices should be inverted: 1/100 and 1/90.
                    // Check for presence of inverted prices, order might not be guaranteed
                    // after potential internal sorting or transformations, though current logic preserves it.
                    let expected_inverted_prices: Vec<Decimal> =
                        vec![dec!(1) / dec!(100), dec!(1) / dec!(90)];
                    let mut reverse_tick_prices: Vec<Decimal> =
                        ticks.iter().map(|t| t.price).collect();
                    reverse_tick_prices
                        .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let mut expected_sorted_inverted_prices = expected_inverted_prices;
                    expected_sorted_inverted_prices
                        .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                    assert_eq!(
                        reverse_tick_prices, expected_sorted_inverted_prices,
                        "Reversed tick prices are incorrect: {:?}",
                        ticks
                    );

                    // Check liquidity gross is preserved for corresponding original ticks (assuming order correspondence for simplicity here)
                    // A more robust check would map original ticks to inverted ones if order isn't guaranteed.
                    assert_eq!(
                        ticks
                            .iter()
                            .find(|t| t.price == dec!(1) / dec!(100))
                            .unwrap()
                            .liquidity_gross,
                        forward_ticks
                            .iter()
                            .find(|t| t.price == dec!(100))
                            .unwrap()
                            .liquidity_gross
                    );
                    assert_eq!(
                        ticks
                            .iter()
                            .find(|t| t.price == dec!(1) / dec!(90))
                            .unwrap()
                            .liquidity_gross,
                        forward_ticks
                            .iter()
                            .find(|t| t.price == dec!(90))
                            .unwrap()
                            .liquidity_gross
                    );
                } else {
                    panic!("Reverse edge not CL");
                }
            }
        }
        assert!(found_forward_cl, "Forward CL edge not found");
        assert!(
            found_reverse_cl,
            "Reverse CL edge not found or model not correctly inverted"
        );
    }
}
