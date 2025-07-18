use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use common::types::{ClmmMarketUpdate, DetectorMessage, MarketUpdate, TokenPair};
use config::Config;
use detector::DetectorService;
use dex_adapters::DexAdapter;
use dex_adapters::{HyperionAdapter, TappAdapter, ThalaAdapter};
use market_data_ingestor::{IndexerProcessorConfig, MarketDataIngestorProcessor};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

/// Command line arguments for arb-bot.
#[derive(Parser, Debug)]
struct Args {
    /// Path to the bot configuration YAML
    #[arg(long, default_value = "config/default.yml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();

    // Load and parse the configuration file
    let config_str = fs::read_to_string(&args.config)?;
    let config: Config = serde_yaml::from_str(&config_str).expect("Failed to parse config");

    // Ensure ingestor config is present
    let ingestor_config = config.ingestor
        .expect("Enhanced ingestor configuration is required. Please add 'ingestor' section to your config file.");

    // Instantiate adapters based on the enhanced configuration
    let mut adapters: HashMap<String, Arc<dyn DexAdapter>> = HashMap::new();
    for adapter_config in &ingestor_config.adapters {
        if !adapter_config.enabled {
            continue; // Skip disabled adapters
        }

        let adapter: Arc<dyn DexAdapter> = match adapter_config.name.as_str() {
            "hyperion" => Arc::new(HyperionAdapter::new(vec![adapter_config.module_address.clone()])),
            "thala" => Arc::new(ThalaAdapter::new(vec![adapter_config.module_address.clone()])),
            "tapp" => Arc::new(TappAdapter::new(vec![adapter_config.module_address.clone()])),
            _ => {
                anyhow::bail!("Unknown adapter: {}", adapter_config.name);
            }
        };

        for event_suffix in adapter_config.events.values() {
            let full_event_type = format!("{}{}", adapter_config.module_address, event_suffix);
            adapters.insert(full_event_type, adapter.clone());
        }
    }

    // Create the MDI config using enhanced configuration
    let mdi_config = IndexerProcessorConfig::from_enhanced_config(
        config.transaction_stream_config,
        ingestor_config,
    );

    // --- New Channel Setup ---
    // Channel for MDI -> Detector communication (block messages + updates)
    let (detector_tx, detector_rx) = mpsc::channel(100);
    // Channel for Detector -> Risk Manager communication
    let (opportunity_tx, mut opportunity_rx) = mpsc::channel(100);

    // --- Instantiate and Spawn Detector Service ---
    let detector_service = DetectorService::new(
        detector_rx,
        opportunity_tx,
        config.detector_config.strategies,
    )?;
    let detector_handle = tokio::spawn(async move { detector_service.run().await });

    // --- Instantiate and Spawn MDI ---
    // The MDI needs a sender for the *broadcast* channel now.
    let (mdi_shutdown_tx, mdi_shutdown_rx) = oneshot::channel();
    let mut mdi = MarketDataIngestorProcessor::new(mdi_config, adapters).await?;
    mdi.set_update_sender(detector_tx.clone());
    mdi.set_shutdown_receiver(mdi_shutdown_rx);
    let mdi_handle = tokio::spawn(async move { mdi.run_processor().await });

    // --- Dummy Message Sending Loop for Testing ---
    let test_sender = detector_tx.clone();
    tokio::spawn(async move {
        let mut block_number = 1;
        loop {
            info!("Sending dummy BlockStart for block {}", block_number);
            test_sender
                .send(DetectorMessage::BlockStart {
                    block_number,
                    timestamp: Utc::now(),
                })
                .await
                .unwrap();

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            info!("Sending dummy MarketUpdate for block {}", block_number);
            test_sender
                .send(DetectorMessage::MarketUpdate(MarketUpdate::Clmm(
                    ClmmMarketUpdate {
                        pool_address: "0x123".to_string(),
                        dex_name: "DummyDex".to_string(),
                        token_pair: TokenPair {
                            token0: "APT".to_string(),
                            token1: "USDC".to_string(),
                        },
                        sqrt_price: 0,
                        liquidity: 0,
                        tick: 0,
                        fee_bps: 0,
                        tick_map: HashMap::new(),
                    },
                )))
                .await
                .unwrap();

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            info!("Sending dummy BlockEnd for block {}", block_number);
            test_sender
                .send(DetectorMessage::BlockEnd { block_number })
                .await
                .unwrap();

            block_number += 1;
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    // --- Dummy Opportunity Receiver for Testing ---
    tokio::spawn(async move {
        while let Some(opportunity) = opportunity_rx.recv().await {
            info!("Received opportunity: {:?}", opportunity);
        }
    });

    tokio::signal::ctrl_c().await?;

    // Graceful shutdown
    info!("Shutting down...");
    mdi_shutdown_tx.send(()).ok();

    if let Err(e) = detector_handle.await.expect("detector task panicked") {
        error!(error = %e, "Detector exited with error");
    }
    if let Err(e) = mdi_handle.await.expect("MDI task panicked") {
        error!(error = %e, "MDI exited with error");
    }

    Ok(())
}
