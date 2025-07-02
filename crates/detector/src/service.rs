use crate::error::DetectorError;
use crate::metrics::DetectorMetrics;
use crate::{
    deduplicator::OpportunityDeduplicator,
    graph::PriceGraph,
    strategies::{create_strategy, ArbitrageStrategy, StrategyConfig},
    transform::transform_update,
};
use anyhow::Result;
use common::types::{ArbitrageOpportunity, DetectorMessage, MarketUpdate, TradingPair};
use futures::future::join_all;
use futures::FutureExt;
use log::{debug, error, info, warn};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// The core service for the arbitrage detector.
pub struct DetectorService {
    /// Receives block-aligned messages from the MDI.
    receiver: mpsc::Receiver<DetectorMessage>,
    /// Sends found arbitrage opportunities to the risk manager.
    opportunity_sender: mpsc::Sender<ArbitrageOpportunity>,
    /// The price graph (shared pointer for cheap cloning).
    price_graph: Arc<PriceGraph>,
    /// The configured arbitrage strategies.
    strategies: Vec<Box<dyn ArbitrageStrategy>>,
    /// The opportunity deduplicator.
    deduplicator: OpportunityDeduplicator,
    /// Tracks trading pairs updated during the current block.
    updated_pairs: HashSet<TradingPair>,
    /// Metrics for error handling and observability.
    metrics: DetectorMetrics,
}

impl DetectorService {
    /// Creates a new `DetectorService`.
    pub fn new(
        receiver: mpsc::Receiver<DetectorMessage>,
        opportunity_sender: mpsc::Sender<ArbitrageOpportunity>,
        strategy_configs: Vec<StrategyConfig>,
    ) -> Result<Self> {
        let strategies = strategy_configs
            .iter()
            .map(create_strategy)
            .collect::<Result<Vec<_>>>()?;
        info!("Loaded {} strategies", strategies.len());

        Ok(Self {
            receiver,
            opportunity_sender,
            price_graph: Arc::new(PriceGraph::new()),
            strategies,
            deduplicator: OpportunityDeduplicator::new(Duration::from_secs(1)),
            updated_pairs: HashSet::new(),
            metrics: DetectorMetrics::new(),
        })
    }

    /// Starts the main service loop.
    pub async fn run(mut self) -> Result<()> {
        info!("DetectorService started.");
        while let Some(message) = self.receiver.recv().await {
            let outcome = std::panic::AssertUnwindSafe(self.handle_message(message))
                .catch_unwind()
                .await;
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(e)) => warn!("Error handling message: {}", e),
                Err(_) => warn!("Panic while handling message"),
            }
        }
        info!("Detector channel closed.");
        Ok(())
    }

    /// Handles a single `DetectorMessage`.
    async fn handle_message(&mut self, message: DetectorMessage) -> Result<()> {
        match message {
            DetectorMessage::BlockStart { .. } => {
                self.updated_pairs.clear();
            }
            DetectorMessage::MarketUpdate(update) => {
                let pool_address = match &update {
                    MarketUpdate::Clmm(data) => &data.pool_address,
                    MarketUpdate::ConstantProduct(data) => &data.pool_address,
                    MarketUpdate::StableSwap(data) => &data.pool_address,
                    MarketUpdate::WeightedPool(data) => &data.pool_address,
                };
                debug!("Received MarketUpdate for pool: {}", pool_address);
                match transform_update(update.clone()) {
                    Ok(edge) => {
                        self.updated_pairs.insert(edge.pair.clone());
                        Arc::make_mut(&mut self.price_graph).update_edge(edge);
                    }
                    Err(e) => {
                        self.handle_error(DetectorError::TransformError(e.to_string()))
                            .await;
                    }
                }
            }
            DetectorMessage::BlockEnd { block_number } => {
                debug!("Received BlockEnd: block_number={}", block_number);
                // Run all strategies for this block
                if let Err(e) = self.detect_all_strategies(block_number).await {
                    self.handle_error(DetectorError::StrategyFailed(
                        "detect_all_strategies".to_string(),
                        e.to_string(),
                    ))
                    .await;
                }
                // Smart prune graph based on activity and TVL
                let stats = Arc::make_mut(&mut self.price_graph).prune();
                info!("Pruned {} edges, retained {}", stats.pruned, stats.retained);
            }
        }
        Ok(())
    }

    /// Runs all configured strategies in parallel.
    async fn detect_all_strategies(&mut self, block_number: u64) -> Result<()> {
        let graph = Arc::clone(&self.price_graph);
        let mut tasks = Vec::with_capacity(self.strategies.len());

        for strategy in &self.strategies {
            let views = strategy.incremental_views(&self.updated_pairs);
            for view in views {
                let strat = strategy.clone_dyn();
                let strat_name = strat.name().to_string();
                let graph = Arc::clone(&graph);
                let task = tokio::spawn(async move {
                    let pv = graph.create_view(&view);
                    let result = strat.detect_opportunities(&pv, block_number).await;
                    (strat_name, result)
                });
                tasks.push(task);
            }
        }

        let results = join_all(tasks).await;

        for result in results {
            match result {
                Ok((_name, Ok(opportunities))) => {
                    for opp in opportunities {
                        if !self.deduplicator.is_duplicate(&opp) {
                            // Use try_send to avoid blocking if the channel is full
                            match self.opportunity_sender.try_send(opp) {
                                Ok(()) => {}
                                Err(mpsc::error::TrySendError::Full(_)) => {
                                    warn!("Opportunity channel full, dropping opportunity");
                                    self.metrics.dropped_opportunities.inc();
                                }
                                Err(mpsc::error::TrySendError::Closed(_)) => {
                                    self.handle_error(DetectorError::ChannelClosed).await;
                                }
                            }
                        }
                    }
                }
                Ok((name, Err(e))) => {
                    self.handle_error(DetectorError::StrategyFailed(name.clone(), e.to_string()))
                        .await;
                }
                Err(e) => {
                    self.handle_error(DetectorError::StrategyFailed(
                        "spawn".to_string(),
                        e.to_string(),
                    ))
                    .await;
                }
            }
        }

        Ok(())
    }

    async fn handle_error(&mut self, error: DetectorError) {
        match error {
            DetectorError::DisconnectedComponent(component_id) => {
                warn!("Disconnected component detected: {:?}", component_id);
                self.metrics.disconnected_components.inc();
            }
            DetectorError::GraphCorruption => {
                error!("Graph corruption detected, rebuilding...");
                self.rebuild_graph_from_cache().await;
            }
            DetectorError::StrategyFailed(name, err) => {
                warn!("Strategy {} failed: {}", name, err);
                self.metrics
                    .strategy_failures
                    .with_label_values(&[&name])
                    .inc();
            }
            _ => {
                error!("Detector error: {:?}", error);
            }
        }
    }

    async fn rebuild_graph_from_cache(&mut self) {
        // Placeholder: reset the graph. Real implementation should replay cached updates.
        self.price_graph = Arc::new(PriceGraph::new());
    }
}

#[cfg(test)]
mod service_tests {
    use super::*;
    use crate::error::DetectorError;
    use crate::graph::{Edge, PoolModel};
    use common::types::{Asset, GraphView, Quantity, TradingPair};
    use rust_decimal_macros::dec;
    use std::str::FromStr;
    use std::time::Instant;
    use tokio::sync::mpsc;

    fn dummy_graph_edge() -> Edge {
        let a = Asset::from_str("A").unwrap();
        let b = Asset::from_str("B").unwrap();
        Edge {
            pair: TradingPair::new(a, b),
            exchange: "dex".to_string(),
            pool_address: "p".to_string(),
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(dec!(1)),
                reserve_y: Quantity(dec!(1)),
                fee_bps: 0,
            },
            last_updated: Instant::now(),
        }
    }

    use crate::graph::AssetId;

    #[tokio::test]
    async fn test_handle_error_disconnected_component() {
        let (_tx, rx) = mpsc::channel(1);
        let (tx2, _rx2) = mpsc::channel(1);
        let mut service = DetectorService::new(rx, tx2, vec![]).unwrap();
        let initial = service.metrics.disconnected_components.get();
        service
            .handle_error(DetectorError::DisconnectedComponent(AssetId::new(0)))
            .await;
        assert_eq!(service.metrics.disconnected_components.get(), initial + 1);
    }

    #[tokio::test]
    async fn test_handle_error_strategy_failed() {
        let (_tx, rx) = mpsc::channel(1);
        let (tx2, _rx2) = mpsc::channel(1);
        let mut service = DetectorService::new(rx, tx2, vec![]).unwrap();
        let counter = service
            .metrics
            .strategy_failures
            .with_label_values(&["test"])
            .get();
        service
            .handle_error(DetectorError::StrategyFailed(
                "test".to_string(),
                "err".to_string(),
            ))
            .await;
        assert_eq!(
            service
                .metrics
                .strategy_failures
                .with_label_values(&["test"])
                .get(),
            counter + 1
        );
    }

    #[tokio::test]
    async fn test_handle_error_graph_corruption_resets_graph() {
        let (_tx, rx) = mpsc::channel(1);
        let (tx2, _rx2) = mpsc::channel(1);
        let mut service = DetectorService::new(rx, tx2, vec![]).unwrap();
        // Insert edge to graph
        Arc::make_mut(&mut service.price_graph).update_edge(dummy_graph_edge());
        let view_before = service.price_graph.create_view(&GraphView::All);
        assert_eq!(view_before.graph.edge_count(), 1);
        service.handle_error(DetectorError::GraphCorruption).await;
        let view_after = service.price_graph.create_view(&GraphView::All);
        assert_eq!(view_after.graph.edge_count(), 0);
    }
}
