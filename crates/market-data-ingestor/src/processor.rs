use crate::{
    config::IndexerProcessorConfig,
    steps::{ClmmParserStep, DetectorPushStep, EventExtractorStep},
    types::MarketUpdate,
};
use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStream;
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct MarketDataIngestorProcessor {
    config: IndexerProcessorConfig,
    update_sender: Option<mpsc::Sender<MarketUpdate>>,
}

impl MarketDataIngestorProcessor {
    pub async fn new(config: IndexerProcessorConfig) -> Result<Self> {
        Ok(Self {
            config,
            update_sender: None,
        })
    }

    /// Set the channel sender for pushing updates to the detector
    pub fn set_update_sender(&mut self, sender: mpsc::Sender<MarketUpdate>) {
        self.update_sender = Some(sender);
    }

    pub async fn run_processor(&self) -> Result<()> {
        info!("Starting Market Data Ingestor processor");

        // Ensure we have a channel to push updates
        let update_sender = self
            .update_sender
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Update sender not set"))?
            .clone();

        // Get starting version
        let starting_version = self.config.transaction_stream_config.starting_version;

        info!(starting_version = ?starting_version, "Starting from version");

        // Create the transaction stream
        let mut transaction_stream = TransactionStream::new(self.config.transaction_stream_config.clone())
            .await?;

        // Create processing steps
        let mut event_extractor = EventExtractorStep::new(self.config.market_data_config.dexs.clone());
        
        let mut clmm_parser = ClmmParserStep::new(
            self.config.market_data_config.dexs.clone(),
        );

        let detector_push = DetectorPushStep::new(update_sender);

        // Main processing loop
        info!("Starting main processing loop");
        
        loop {
            match transaction_stream.get_next_transaction_batch().await {
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
                        match event_extractor.process_transaction(transaction).await {
                            Ok(events) if !events.is_empty() => {
                                // Parse events into market updates
                                match clmm_parser.process_events(events).await {
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

        Ok(())
    }
}
