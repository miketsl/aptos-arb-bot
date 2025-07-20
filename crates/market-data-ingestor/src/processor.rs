use crate::data_source::{DataSource as SourceTrait, FileSource, GrpcSource};
use crate::ingestor_config::IndexerProcessorConfig;
use crate::steps::{DetectorPushStep, EventExtractorStep, FilterStep, Parser};
use anyhow::Result;
use chrono::Utc;
use common::types::DetectorMessage;
use config_lib::DataSourceConfig;
use dex_adapters::DexAdapter;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

pub struct MarketDataIngestorProcessor {
    config: IndexerProcessorConfig,
    parser: Parser,
    filter_step: FilterStep,
    update_sender: Option<mpsc::Sender<DetectorMessage>>,
    shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl MarketDataIngestorProcessor {
    pub async fn new(
        config: IndexerProcessorConfig,
        adapters: HashMap<String, Arc<dyn DexAdapter>>,
    ) -> Result<Self> {
        let parser = Parser::new(adapters);
        let filter_step = FilterStep::from_ingestor_config(&config.ingestor_config.filters);
        Ok(Self {
            config,
            parser,
            filter_step,
            update_sender: None,
            shutdown_rx: None,
        })
    }

    /// Set the channel sender for pushing updates to the detector
    /// Set the channel sender for pushing detector messages (BlockStart/Updates/BlockEnd)
    pub fn set_update_sender(&mut self, sender: mpsc::Sender<DetectorMessage>) {
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

        // Select data source (gRPC live stream or file replay) from config
        let mut source: Box<dyn SourceTrait> = match &self.config.ingestor_config.data_source {
            DataSourceConfig::Grpc {
                endpoint: _,
                timeout_ms: _,
                ..
            } => Box::new(GrpcSource::new(
                self.config.transaction_stream_config.clone(),
            )),
            DataSourceConfig::File {
                path, replay_speed, ..
            } => {
                let speed = replay_speed.unwrap_or(1.0);
                Box::new(FileSource::new(path.clone(), speed))
            }
        };

        // Start the data source
        source
            .start()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start data source: {}", e))?;

        // Create processing steps
        let mut event_extractor =
            EventExtractorStep::from_adapter_configs(self.config.ingestor_config.adapters.clone());

        let detector_push = DetectorPushStep::new(update_sender);

        info!("Starting main processing loop");

        loop {
            tokio::select! {
                biased;

                _ = &mut shutdown_rx => {
                    warn!("Shutdown signal received. Exiting MDI processing loop.");
                    break;
                }

                event_result = source.next_event() => {
                    match event_result {
                        Ok(Some(timestamped_event)) => {
                            let transaction = timestamped_event.raw_event.transaction;
                            let version = transaction.version;

                            info!(
                                version = version,
                                sequence = timestamped_event.sequence,
                                "Received transaction event"
                            );

                            // Emit BlockStart before processing this transaction
                            detector_push.push(DetectorMessage::BlockStart {
                                block_number: version,
                                timestamp: Utc::now(),
                            }).await?;

                            // Extract relevant events
                            match event_extractor.process_transaction(transaction.clone()).await {
                                Ok(events) if !events.is_empty() => {
                                    // Parse events into market updates
                                    match self.parser.process_events(&events) {
                                        Ok(mut updates) if !updates.is_empty() => {
                                            // Filter in-place to drop unwanted pools
                                            self.filter_step.apply(&mut updates);
                                            if !updates.is_empty() {
                                                // Push updates to detector
                                                for update in updates {
                                                    if let Err(e) = detector_push.push(DetectorMessage::MarketUpdate(update)).await {
                                                        error!(version = version, error = %e, "Failed to send MarketUpdate message");
                                                    }
                                                }
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

                            // Emit BlockEnd after processing this transaction
                            detector_push.push(DetectorMessage::BlockEnd { block_number: version }).await?;
                        }
                        Ok(None) => {
                            info!("Data source ended");
                            break;
                        }
                        Err(e) => {
                            error!(error = %e, "Error receiving transaction event");
                            break;
                        }
                    }
                }
            }
        }

        // Clean up the data source
        if let Err(e) = source.stop().await {
            warn!(error = %e, "Error stopping data source");
        }

        Ok(())
    }
}
