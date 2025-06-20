use crate::ingestor_config::IndexerProcessorConfig;
use crate::steps::{DetectorPushStep, EventExtractorStep, Parser};
use crate::types::MarketUpdate;
use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStream;
use dex_adapter_trait::DexAdapter;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

pub struct MarketDataIngestorProcessor {
    config: IndexerProcessorConfig,
    parser: Parser,
    update_sender: Option<mpsc::Sender<MarketUpdate>>,
    shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl MarketDataIngestorProcessor {
    pub async fn new(
        config: IndexerProcessorConfig,
        adapters: HashMap<String, Arc<dyn DexAdapter>>,
    ) -> Result<Self> {
        let parser = Parser::new(adapters);
        Ok(Self {
            config,
            parser,
            update_sender: None,
            shutdown_rx: None,
        })
    }

    /// Set the channel sender for pushing updates to the detector
    pub fn set_update_sender(&mut self, sender: mpsc::Sender<MarketUpdate>) {
        self.update_sender = Some(sender);
    }

    /// Set the shutdown receiver for graceful shutdown
    pub fn set_shutdown_receiver(&mut self, shutdown_rx: oneshot::Receiver<()>) {
        self.shutdown_rx = Some(shutdown_rx);
    }

    pub async fn run_processor(mut self) -> Result<()> {
        info!("Starting Market Data Ingestor processor");

        let update_sender = self
            .update_sender
            .take()
            .ok_or_else(|| anyhow::anyhow!("Update sender not set"))?;

        let mut shutdown_rx = self
            .shutdown_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Shutdown receiver not set"))?;

        let starting_version = self.config.transaction_stream_config.starting_version;
        info!(starting_version = ?starting_version, "Starting from version");

        let mut transaction_stream =
            TransactionStream::new(self.config.transaction_stream_config.clone()).await?;

        // Create processing steps
        let mut event_extractor =
            EventExtractorStep::new(self.config.market_data_config.dexs.clone());

        let detector_push = DetectorPushStep::new(update_sender);

        info!("Starting main processing loop");

        loop {
            tokio::select! {
                biased;

                _ = &mut shutdown_rx => {
                    warn!("Shutdown signal received. Exiting MDI processing loop.");
                    break;
                }

                batch_result = transaction_stream.get_next_transaction_batch() => {
                    match batch_result {
                        Ok(response) => {
                            info!(
                                start_version = response.start_version,
                                end_version = response.end_version,
                                num_transactions = response.transactions.len(),
                                "Received transaction batch"
                            );
                            for transaction in response.transactions {
                                let version = transaction.version;

                                // Extract relevant events
                                match event_extractor.process_transaction(transaction.clone()).await {
                                    Ok(events) if !events.is_empty() => {
                                        // Parse events into market updates
                                        match self.parser.process_events(&events) {
                                            Ok(updates) if !updates.is_empty() => {
                                                // Push updates to detector
                                                if let Err(e) = detector_push.push_updates(updates).await {
                                                    error!(version = version, error = %e, "Failed to push updates");
                                                }
                                            }
                                            Ok(_) => {} // No updates generated
                                            Err(e) => {
                                                error!(version = version, error = %e, "Failed to parse events");
                                            }
                                        }
                                    }
                                    Ok(_) => {} // No relevant events
                                    Err(e) => {
                                        error!(version = version, error = %e, "Failed to extract events");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Error receiving transaction batch");
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
