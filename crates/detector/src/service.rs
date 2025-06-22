use crate::{
    deduplicator::OpportunityDeduplicator,
    graph::PriceGraph,
    strategies::{create_strategy, ArbitrageStrategy, StrategyConfig},
    transform::transform_update,
};
use anyhow::Result;
use common::types::{ArbitrageOpportunity, DetectorMessage, GraphView, TradingPair};
use futures::future::join_all;
use log::{debug, info, warn};
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
        })
    }

    /// Starts the main service loop.
    pub async fn run(mut self) -> Result<()> {
        info!("DetectorService started.");
        while let Some(message) = self.receiver.recv().await {
            if let Err(e) = self.handle_message(message).await {
                warn!("Error handling message: {}", e);
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
                debug!("Received MarketUpdate for pool: {}", update.pool_address);
                if let Ok(edge) = transform_update(update) {
                    self.updated_pairs.insert(edge.pair.clone());
                    Arc::make_mut(&mut self.price_graph).update_edge(edge);
                }
            }
            DetectorMessage::BlockEnd { block_number } => {
                debug!("Received BlockEnd: block_number={}", block_number);
                // Run all strategies for this block
                self.detect_all_strategies(block_number).await?;
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
            let name = strategy.name();
            let req_view = strategy.required_graph_view();
            // Incremental: cross-DEX strategy runs per updated pair
            if name == "cross_dex_arbitrage" {
                for pair in &self.updated_pairs {
                    let strat = strategy.clone_dyn();
                    let graph = Arc::clone(&graph);
                    let pair = pair.clone();
                    let task = tokio::spawn(async move {
                        let view = graph.create_view(&GraphView::PairFiltered(pair));
                        strat.detect_opportunities(&view, block_number).await
                    });
                    tasks.push(task);
                }
            } else {
                let strat = strategy.clone_dyn();
                let graph = Arc::clone(&graph);
                let task = tokio::spawn(async move {
                    let view = graph.create_view(&req_view);
                    strat.detect_opportunities(&view, block_number).await
                });
                tasks.push(task);
            }
        }

        let results = join_all(tasks).await;

        for result in results {
            match result {
                Ok(Ok(opportunities)) => {
                    for opp in opportunities {
                        if !self.deduplicator.is_duplicate(&opp) {
                            if let Err(e) = self.opportunity_sender.send(opp).await {
                                warn!("Failed to send opportunity: {}", e);
                            }
                        }
                    }
                }
                Ok(Err(e)) => warn!("Strategy failed: {}", e),
                Err(e) => warn!("Strategy task failed: {}", e),
            }
        }

        Ok(())
    }
}
