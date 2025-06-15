use crate::prelude::*;
use common::types::{Asset, ExchangeId, Quantity, TradingPair};
use petgraph::graphmap::DiGraphMap;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
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
    pub exchange: ExchangeId,
    pub model: PoolModel,
    pub last_updated: Instant,
}

// Manual PartialEq implementation for Edge if needed due to Instant, though for graph weights it's not strictly necessary for map keys.
// For now, relying on derive(Clone) and using it as a weight.
// If Edge were a node, it would need Eq and Hash.
impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        self.pair == other.pair &&
        self.exchange == other.exchange &&
        self.model == other.model &&
        self.last_updated == other.last_updated // Note: Instant comparison is fine
    }
}


impl Edge {
    /// Calculates the output amount for a given input amount based on the pool model.
    pub fn quote(&self, amount_in: &Quantity) -> Option<Quantity> {
        match &self.model {
            PoolModel::ConstantProduct { reserve_x, reserve_y, fee_bps } => {
                // For tuple struct Quantity, we can't check asset directly
                // The caller must ensure the asset matches

                if reserve_x.0.is_zero() || reserve_y.0.is_zero() || amount_in.0.is_zero() {
                    return None;
                }

                let amount_in_val = amount_in.0;
                let reserve_x_val = reserve_x.0;
                let reserve_y_val = reserve_y.0;

                let fee_decimal = Decimal::from_u16(*fee_bps).unwrap_or_default() / Decimal::new(10000, 0);
                let amount_in_after_fee = amount_in_val * (Decimal::ONE - fee_decimal);

                // Classic CPMM: (x + dx_eff) * (y - dy) = x * y
                // dy = y * dx_eff / (x + dx_eff)
                let amount_out_val = (reserve_y_val * amount_in_after_fee) / (reserve_x_val + amount_in_after_fee);

                if amount_out_val.is_sign_negative() || amount_out_val.is_zero() || amount_out_val > reserve_y_val {
                    return None; // Not enough liquidity or invalid amount
                }
                Some(Quantity(amount_out_val))
            }
            PoolModel::ConcentratedLiquidity { .. } => {
                // Complex logic, placeholder
                None
            }
        }
    }
}

/// A snapshot of the price graph, which is an immutable copy.
#[derive(Clone, Debug)]
pub struct PriceGraphSnapshot {
    graph: DiGraphMap<AssetId, Edge>,
    asset_mapping: HashMap<AssetId, Asset>,
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
pub trait PriceGraph {
    /// Inserts or updates an edge in the graph.
    /// An edge represents a one-way path from `edge.pair.asset_x` to `edge.pair.asset_y`.
    fn upsert_edge(&mut self, edge: Edge);

    /// Ingests a batch of edges.
    fn ingest_batch(&mut self, edges: Vec<Edge>);

    /// Removes edges that haven't been updated within the given `ttl`.
    fn prune_stale(&mut self, ttl: Duration);

    /// Returns an iterator over the neighbors of a given asset and the edges leading to them.
    /// Neighbors are assets reachable directly from the given `asset`.
    fn neighbors<'a>(&'a self, asset: &Asset) -> Box<dyn Iterator<Item = (&'a Asset, &'a Edge)> + 'a>;

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
    fn upsert_edge(&mut self, edge: Edge) {
        let source_id = self.get_or_create_asset_id(&edge.pair.asset_x);
        let target_id = self.get_or_create_asset_id(&edge.pair.asset_y);
        self.graph.add_edge(source_id, target_id, edge);
    }

    fn ingest_batch(&mut self, edges: Vec<Edge>) {
        for edge in edges {
            self.upsert_edge(edge);
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

    fn neighbors<'a>(&'a self, asset: &Asset) -> Box<dyn Iterator<Item = (&'a Asset, &'a Edge)> + 'a> {
        if let Some(&asset_id) = self.reverse_mapping.get(asset) {
            Box::new(
                self.graph
                    .edges(asset_id)
                    .map(|(_, target_id, edge_data)| (&self.asset_mapping[&target_id], edge_data))
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
    use common::types::{Asset, ExchangeId, Quantity, TradingPair};
    use rust_decimal_macros::dec;
    use std::str::FromStr;


    fn usdc_asset() -> Asset { Asset::from_str("0x1::coin::USDC").unwrap() }
    fn apt_asset() -> Asset { Asset::from_str("0x1::aptos_coin::AptosCoin").unwrap() }
    fn eth_asset() -> Asset { Asset::from_str("0x1::coin::ETH").unwrap() }


    fn create_test_edge(asset_x: Asset, asset_y: Asset, reserve_x_val: Decimal, reserve_y_val: Decimal, fee_bps: u16) -> Edge {
        Edge {
            pair: TradingPair {
                asset_x: asset_x.clone(),
                asset_y: asset_y.clone(),
            },
            exchange: ExchangeId::pancakeswap_v3(),
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
        
        graph.upsert_edge(edge1.clone());
        
        let snapshot = graph.snapshot();
        assert_eq!(snapshot.edge_count(), 1);
        assert_eq!(snapshot.node_count(), 2);
        
        let (_, _, retrieved_edge) = snapshot.all_edges().next().unwrap();
        assert_eq!(retrieved_edge.pair, edge1.pair);
    }

    #[test]
    fn test_ingest_batch() {
        let mut graph = PriceGraphImpl::new();
        let edge1 = create_test_edge(usdc_asset(), apt_asset(), dec!(1000), dec!(100), 30);
        let edge2 = create_test_edge(apt_asset(), eth_asset(), dec!(50), dec!(1), 30);
        
        graph.ingest_batch(vec![edge1, edge2]);
        
        let snapshot = graph.snapshot();
        assert_eq!(snapshot.edge_count(), 2);
        assert_eq!(snapshot.node_count(), 3);
    }

    #[test]
    fn test_prune_stale() {
        let mut graph = PriceGraphImpl::new();
        let fresh_edge = create_test_edge(usdc_asset(), apt_asset(), dec!(1000), dec!(100), 30);
        let mut stale_edge = create_test_edge(apt_asset(), eth_asset(), dec!(50), dec!(1), 30);
        
        stale_edge.last_updated = Instant::now() - Duration::from_secs(10);
        
        graph.upsert_edge(fresh_edge.clone());
        graph.upsert_edge(stale_edge.clone());
        
        assert_eq!(graph.snapshot().edge_count(), 2);
        
        graph.prune_stale(Duration::from_secs(5));
        let snapshot = graph.snapshot();
        assert_eq!(snapshot.edge_count(), 1);

        // Ensure the fresh edge remains
        let (_, _, remaining_edge) = snapshot.all_edges().next().unwrap();
        assert_eq!(remaining_edge.pair, fresh_edge.pair);
    }

    #[test]
    fn test_neighbors() {
        let mut graph = PriceGraphImpl::new();
        let edge_usdc_apt = create_test_edge(usdc_asset(), apt_asset(), dec!(1000), dec!(100), 30);
        let edge_usdc_eth = create_test_edge(usdc_asset(), eth_asset(), dec!(2000), dec!(1), 25);
        let edge_apt_eth = create_test_edge(apt_asset(), eth_asset(), dec!(50), dec!(0.5), 30);

        graph.upsert_edge(edge_usdc_apt.clone());
        graph.upsert_edge(edge_usdc_eth.clone());
        graph.upsert_edge(edge_apt_eth.clone());

        let usdc_neighbors: Vec<_> = graph.neighbors(&usdc_asset()).collect();
        assert_eq!(usdc_neighbors.len(), 2);
        
        // Check if APT and ETH are neighbors of USDC
        let has_apt_neighbor = usdc_neighbors.iter().any(|(asset, edge)| *asset == &apt_asset() && edge.pair == edge_usdc_apt.pair);
        let has_eth_neighbor = usdc_neighbors.iter().any(|(asset, edge)| *asset == &eth_asset() && edge.pair == edge_usdc_eth.pair);
        assert!(has_apt_neighbor);
        assert!(has_eth_neighbor);

        let apt_neighbors: Vec<_> = graph.neighbors(&apt_asset()).collect();
        assert_eq!(apt_neighbors.len(), 1);
        assert_eq!(apt_neighbors[0].0, &eth_asset());
        assert_eq!(apt_neighbors[0].1.pair, edge_apt_eth.pair);

        let eth_neighbors: Vec<_> = graph.neighbors(&eth_asset()).collect();
        assert_eq!(eth_neighbors.len(), 0);
    }
    
    #[test]
    fn test_edge_quote_simple_cpmm() {
        // USDC -> APT pool
        let edge = create_test_edge(usdc_asset(), apt_asset(), dec!(10000), dec!(1000), 25); // 0.25% fee

        // Quote selling 100 USDC for APT
        let amount_in_usdc = Quantity { asset: usdc_asset(), amount: dec!(100) };
        let quote_result = edge.quote(&amount_in_usdc);

        assert!(quote_result.is_some());
        let amount_out_apt = quote_result.unwrap();
        assert_eq!(amount_out_apt.asset, apt_asset());

        // Manual calculation:
        // fee = 100 * (25/10000) = 0.25 USDC
        // amount_in_after_fee = 100 - 0.25 = 99.75 USDC
        // reserve_x = 10000, reserve_y = 1000
        // amount_out = reserve_y * amount_in_after_fee / (reserve_x + amount_in_after_fee)
        // amount_out = 1000 * 99.75 / (10000 + 99.75)
        // amount_out = 99750 / 10099.75 = 9.876479...
        let expected_out = dec!(1000) * (dec!(100) * (Decimal::ONE - dec!(0.0025))) / (dec!(10000) + (dec!(100) * (Decimal::ONE - dec!(0.0025))));
        assert_eq!(amount_out_apt.amount.round_dp(8), expected_out.round_dp(8));

        // Quote selling asset not in pair (APT for APT)
        let amount_in_apt_wrong = Quantity { asset: apt_asset(), amount: dec!(10) };
        assert!(edge.quote(&amount_in_apt_wrong).is_none());
         // Quote selling asset that is asset_y (APT for USDC, but edge is USDC -> APT)
        let amount_in_apt_reverse = Quantity { asset: apt_asset(), amount: dec!(10) };
        assert!(edge.quote(&amount_in_apt_reverse).is_none());
    }

     #[test]
    fn test_edge_quote_insufficient_liquidity() {
        let edge = create_test_edge(usdc_asset(), apt_asset(), dec!(100), dec!(10), 25);
        
        // Try to swap more than available in reserve_x after fee
        let large_amount_in = Quantity { asset: usdc_asset(), amount: dec!(10000) };
        let quote_result = edge.quote(&large_amount_in);
        // The formula itself might not show this as an error directly unless output is > reserve_y
        // Let's test if output is greater than reserve_y
        // amount_in_after_fee = 10000 * (1 - 0.0025) = 9975
        // amount_out = 10 * 9975 / (100 + 9975) = 99750 / 10075 = 9.9007...
        // This is valid and < 10.
        assert!(quote_result.is_some());
        assert!(quote_result.unwrap().amount <= dec!(10));


        // Test with zero reserve
        let zero_reserve_edge = create_test_edge(usdc_asset(), apt_asset(), dec!(0), dec!(10), 25);
        let amount_in = Quantity { asset: usdc_asset(), amount: dec!(10) };
        assert!(zero_reserve_edge.quote(&amount_in).is_none());

        let zero_reserve_edge_y = create_test_edge(usdc_asset(), apt_asset(), dec!(10), dec!(0), 25);
        assert!(zero_reserve_edge_y.quote(&amount_in).is_none());
    }
}