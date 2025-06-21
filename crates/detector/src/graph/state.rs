use crate::graph::{AssetId, Edge};
use common::types::{Asset, GraphView};
use petgraph::graphmap::DiGraphMap;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A view into the price graph, tailored to the needs of a specific strategy.
#[derive(Debug)]
pub struct PriceGraphView<'a> {
    /// The underlying graph data.
    pub graph: &'a DiGraphMap<AssetId, Edge>,
    /// A map from asset IDs to asset definitions.
    pub asset_mapping: &'a HashMap<AssetId, Asset>,
}

/// The main price graph, responsible for storing and managing market data.
#[derive(Clone, Debug)]
pub struct PriceGraph {
    graph: DiGraphMap<AssetId, Edge>,
    asset_mapping: HashMap<AssetId, Asset>,
    reverse_mapping: HashMap<Asset, AssetId>,
    next_id: u64,
}

impl PriceGraph {
    /// Creates a new, empty price graph.
    pub fn new() -> Self {
        PriceGraph {
            graph: DiGraphMap::new(),
            asset_mapping: HashMap::new(),
            reverse_mapping: HashMap::new(),
            next_id: 0,
        }
    }

    /// Adds or updates an edge in the graph.
    pub fn update_edge(&mut self, edge: Edge) {
        let source_id = self.get_or_create_asset_id(&edge.pair.asset_x);
        let target_id = self.get_or_create_asset_id(&edge.pair.asset_y);
        self.graph.add_edge(source_id, target_id, edge);
    }

    /// Creates a view of the graph, filtered according to the specified criteria.
    pub fn create_view(&self, view: &GraphView) -> PriceGraphView {
        match view {
            GraphView::All => PriceGraphView {
                graph: &self.graph,
                asset_mapping: &self.asset_mapping,
            },
            GraphView::PairFiltered(pair) => {
                // For now, we return the full graph and let the strategy filter.
                // In the future, we could create a subgraph here for efficiency.
                let _pair = pair;
                PriceGraphView {
                    graph: &self.graph,
                    asset_mapping: &self.asset_mapping,
                }
            }
        }
    }

    /// Removes edges that have not been updated within the given duration.
    /// TODO: Extend this to prune based on other metrics like low TVL, etc.
    pub fn prune_stale(&mut self, max_age: Duration) {
        let now = Instant::now();
        let stale_edges: Vec<(AssetId, AssetId)> = self
            .graph
            .all_edges()
            .filter_map(|(source, target, edge)| {
                if now.duration_since(edge.last_updated) > max_age {
                    Some((source, target))
                } else {
                    None
                }
            })
            .collect();

        for (source, target) in stale_edges {
            self.graph.remove_edge(source, target);
        }
    }

    /// Gets the ID for an asset, creating a new one if it doesn't exist.
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

impl Default for PriceGraph {
    fn default() -> Self {
        Self::new()
    }
}
