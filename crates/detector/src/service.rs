use crate::transform::transform_update;
use anyhow::Result;
use common::types::{ArbitrageOpportunity, DetectorMessage};
use log::{debug, info, warn};
use tokio::sync::{broadcast, mpsc};

/// The core service for the arbitrage detector.
pub struct DetectorService {
    /// Receives block-aligned messages from the MDI.
    receiver: broadcast::Receiver<DetectorMessage>,
    /// Sends found arbitrage opportunities to the risk manager.
    #[allow(dead_code)]
    opportunity_sender: mpsc::Sender<ArbitrageOpportunity>,
}

impl DetectorService {
    /// Creates a new `DetectorService`.
    pub fn new(
        receiver: broadcast::Receiver<DetectorMessage>,
        opportunity_sender: mpsc::Sender<ArbitrageOpportunity>,
    ) -> Self {
        Self {
            receiver,
            opportunity_sender,
        }
    }

    /// Starts the main service loop.
    pub async fn run(mut self) -> Result<()> {
        info!("DetectorService started.");
        loop {
            match self.receiver.recv().await {
                Ok(message) => {
                    self.handle_message(message).await?;
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
            DetectorMessage::BlockStart {
                block_number,
                timestamp,
            } => {
                debug!(
                    "Received BlockStart: block_number={}, timestamp={}",
                    block_number, timestamp
                );
                // TODO: Initialize block-specific state
            }
            DetectorMessage::MarketUpdate(update) => {
                debug!("Received MarketUpdate for pool: {}", update.pool_address);
                match transform_update(update) {
                    Ok(_edge) => {
                        // TODO: Update graph with the new edge
                    }
                    Err(e) => {
                        warn!("Failed to transform market update: {}", e);
                    }
                }
            }
            DetectorMessage::BlockEnd { block_number } => {
                debug!("Received BlockEnd: block_number={}", block_number);
                // TODO: Run detection algorithms
                // TODO: Send opportunities via self.opportunity_sender
            }
        }
        Ok(())
    }
}
