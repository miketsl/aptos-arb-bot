use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use common::types::{DetectorMessage, MarketUpdate, TokenPair};
use config::Config;
use detector::DetectorService;
use dex_adapter_trait::DexAdapter;
use dex_adapters::{HyperionAdapter, TappAdapter, ThalaAdapter};
use market_data_ingestor::{IndexerProcessorConfig, MarketDataIngestorProcessor};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot};
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

    // Instantiate adapters based on the configuration
    let mut adapters: HashMap<String, Arc<dyn DexAdapter>> = HashMap::new();
    for dex_config in &config.market_data_config.dexs {
        let adapter: Arc<dyn DexAdapter> = match dex_config.name.as_str() {
            "Hyperion" => Arc::new(HyperionAdapter::new()),
            "ThalaSwap" => Arc::new(ThalaAdapter::new()),
            "Tapp" => Arc::new(TappAdapter),
            _ => {
                anyhow::bail!("Unknown adapter: {}", dex_config.name);
            }
        };

        for event_suffix in dex_config.events.values() {
            let full_event_type = format!("{}{}", dex_config.module_address, event_suffix);
            adapters.insert(full_event_type, adapter.clone());
        }
    }

    // Create the MDI config
    let mdi_config =
        IndexerProcessorConfig::new(config.transaction_stream_config, config.market_data_config);

    // --- New Channel Setup ---
    // Channel for MDI -> Detector communication
    let (detector_tx, detector_rx) = broadcast::channel(100);
    // Channel for Detector -> Risk Manager communication
    let (opportunity_tx, mut opportunity_rx) = mpsc::channel(100);

    // --- Instantiate and Spawn Detector Service ---
    let detector_service = DetectorService::new(detector_rx, opportunity_tx);
    let detector_handle = tokio::spawn(async move { detector_service.run().await });

    // --- Instantiate and Spawn MDI ---
    // The MDI needs a sender for the *broadcast* channel now.
    let _mdi_sender = detector_tx.clone();
    let (mdi_shutdown_tx, mdi_shutdown_rx) = oneshot::channel();
    let mut mdi = MarketDataIngestorProcessor::new(mdi_config, adapters).await?;
    // TODO: The MDI needs to be updated to send DetectorMessage enums instead of just MarketUpdates.
    // For now, we will not connect it.
    // mdi.set_update_sender(mdi_sender);
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
                .unwrap();

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            info!("Sending dummy MarketUpdate for block {}", block_number);
            test_sender
                .send(DetectorMessage::MarketUpdate(MarketUpdate {
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
                }))
                .unwrap();

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            info!("Sending dummy BlockEnd for block {}", block_number);
            test_sender
                .send(DetectorMessage::BlockEnd { block_number })
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
