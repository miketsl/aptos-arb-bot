use crate::graph::{AssetId, Edge};
use common::types::{Asset, GraphView};
use petgraph::graphmap::DiGraphMap;
use rust_decimal::Decimal;
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

    /// Activity stats for each edge (source,target) in the graph.
    edge_activity: HashMap<(AssetId, AssetId), ActivityStats>,

    /// Pruning configuration parameters.
    pruning_config: PruningConfig,
}

impl PriceGraph {
    /// Creates a new, empty price graph.
    pub fn new() -> Self {
        PriceGraph {
            graph: DiGraphMap::new(),
            asset_mapping: HashMap::new(),
            reverse_mapping: HashMap::new(),
            next_id: 0,
            edge_activity: HashMap::new(),
            pruning_config: PruningConfig::default(),
        }
    }

    /// Adds or updates an edge in the graph.
    pub fn update_edge(&mut self, edge: Edge) {
        let source_id = self.get_or_create_asset_id(&edge.pair.asset_x);
        let target_id = self.get_or_create_asset_id(&edge.pair.asset_y);
        self.graph.add_edge(source_id, target_id, edge.clone());
        // Record update timestamp and TVL for pruning metrics
        let stats = self
            .edge_activity
            .entry((source_id, target_id))
            .or_insert_with(ActivityStats::new);
        stats.last_update = Instant::now();
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

    /// Prunes edges according to opportunity, TVL, staleness, and protected pairs.
    pub fn prune(&mut self) -> PruneStats {
        let now = Instant::now();
        let mut pruned = 0;
        let mut retained = 0;

        for (&(source, target), stats) in &self.edge_activity {
            let keep = stats
                .last_opportunity
                .map(|t| now.duration_since(t) < self.pruning_config.opportunity_window)
                .unwrap_or(false)
                || stats.tvl >= self.pruning_config.min_tvl
                || now.duration_since(stats.last_update) < self.pruning_config.max_stale_age
                || {
                    let a = &self.asset_mapping[&source].0;
                    let b = &self.asset_mapping[&target].0;
                    let key = if a < b {
                        (a.clone(), b.clone())
                    } else {
                        (b.clone(), a.clone())
                    };
                    self.pruning_config.protected_pairs.contains(&key)
                };

            if keep {
                retained += 1;
            } else {
                pruned += 1;
                self.graph.remove_edge(source, target);
            }
        }

        PruneStats { pruned, retained }
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

/// Statistics for each edge, used to decide pruning.
#[derive(Clone, Debug)]
pub struct ActivityStats {
    pub last_update: Instant,
    pub last_opportunity: Option<Instant>,
    pub opportunity_count: u32,
    pub total_volume: Decimal,
    pub tvl: Decimal,
}

impl ActivityStats {
    pub fn new() -> Self {
        ActivityStats {
            last_update: Instant::now(),
            last_opportunity: None,
            opportunity_count: 0,
            total_volume: Decimal::ZERO,
            tvl: Decimal::ZERO,
        }
    }
}

/// Configuration parameters controlling graph pruning behavior.
#[derive(Clone, Debug)]
pub struct PruningConfig {
    pub opportunity_window: Duration,
    pub min_tvl: Decimal,
    pub max_stale_age: Duration,
    pub protected_pairs: Vec<(String, String)>,
}

impl Default for PruningConfig {
    fn default() -> Self {
        PruningConfig {
            opportunity_window: Duration::from_secs(3600),
            min_tvl: Decimal::ZERO,
            max_stale_age: Duration::from_secs(300),
            protected_pairs: Vec::new(),
        }
    }
}

/// Summary of a pruning pass.
#[derive(Clone, Debug)]
pub struct PruneStats {
    pub pruned: usize,
    pub retained: usize,
}
