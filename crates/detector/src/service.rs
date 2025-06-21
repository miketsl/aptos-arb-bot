use crate::{
    deduplicator::OpportunityDeduplicator,
    graph::PriceGraph,
    strategies::{create_strategy, ArbitrageStrategy, StrategyConfig},
    transform::transform_update,
};
use anyhow::Result;
use common::types::{ArbitrageOpportunity, DetectorMessage};
use futures::future::join_all;
use log::{debug, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

/// The core service for the arbitrage detector.
pub struct DetectorService {
    /// Receives block-aligned messages from the MDI.
    receiver: broadcast::Receiver<DetectorMessage>,
    /// Sends found arbitrage opportunities to the risk manager.
    opportunity_sender: mpsc::Sender<ArbitrageOpportunity>,
    /// The price graph.
    price_graph: PriceGraph,
    /// The configured arbitrage strategies.
    strategies: Vec<Box<dyn ArbitrageStrategy>>,
    /// The opportunity deduplicator.
    deduplicator: OpportunityDeduplicator,
}

impl DetectorService {
    /// Creates a new `DetectorService`.
    pub fn new(
        receiver: broadcast::Receiver<DetectorMessage>,
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
            price_graph: PriceGraph::new(),
            strategies,
            deduplicator: OpportunityDeduplicator::new(Duration::from_secs(1)),
        })
    }

    /// Starts the main service loop.
    pub async fn run(mut self) -> Result<()> {
        info!("DetectorService started.");
        loop {
            match self.receiver.recv().await {
                Ok(message) => {
                    if let Err(e) = self.handle_message(message).await {
                        warn!("Error handling message: {}", e);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Detector channel lagged by {} messages.", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Detector channel closed.");
                    break;
                }
            }
        }
        Ok(())
    }

    /// Handles a single `DetectorMessage`.
    async fn handle_message(&mut self, message: DetectorMessage) -> Result<()> {
        match message {
            DetectorMessage::BlockStart { .. } => {
                // Not used in this phase
            }
            DetectorMessage::MarketUpdate(update) => {
                debug!("Received MarketUpdate for pool: {}", update.pool_address);
                match transform_update(update) {
                    Ok(edge) => {
                        self.price_graph.update_edge(edge);
                    }
                    Err(e) => {
                        warn!("Failed to transform market update: {}", e);
                    }
                }
            }
            DetectorMessage::BlockEnd { block_number } => {
                debug!("Received BlockEnd: block_number={}", block_number);
                self.detect_all_strategies(block_number).await?;
            }
        }
        Ok(())
    }

    /// Runs all configured strategies in parallel.
    async fn detect_all_strategies(&mut self, block_number: u64) -> Result<()> {
        let graph = Arc::new(self.price_graph.clone());
        let mut tasks = vec![];

        for strategy in &self.strategies {
            let strategy = strategy.clone_dyn();
            let graph = Arc::clone(&graph);
            let task = tokio::spawn(async move {
                let view = graph.create_view(&strategy.required_graph_view());
                strategy.detect_opportunities(&view, block_number).await
            });
            tasks.push(task);
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
