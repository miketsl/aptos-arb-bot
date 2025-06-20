use anyhow::Result;
use clap::Parser;
use config::Config;
use detector::traits::{IsExecutor, IsRiskManager};
use detector::{bellman_ford::DetectorConfig, Detector};
use dex_adapter_trait::DexAdapter;
use dex_adapters::{HyperionAdapter, TappAdapter, ThalaAdapter};
use executor::{BlockchainClient, TradeExecutor};
use market_data_ingestor::{IndexerProcessorConfig, MarketDataIngestorProcessor};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tracing::error;

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
            "Hyperion" => Arc::new(HyperionAdapter),
            "ThalaSwap" => Arc::new(ThalaAdapter),
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

    // Channel between the ingestor and the detector
    let (update_tx, update_rx) = mpsc::channel(100);
    let price_stream = Box::pin(ReceiverStream::new(update_rx));

    // Instantiate core services
    let risk_manager: Arc<dyn IsRiskManager> = Arc::new(core::DummyRiskManager::new());
    let executor: Arc<dyn IsExecutor> = Arc::new(TradeExecutor::new(BlockchainClient::new(None)));
    let dex_adapters_for_detector = detector::service::DexAdapters::new();

    let (detector_shutdown_tx, detector_shutdown_rx) = mpsc::channel(1);
    let detector = Detector::new(
        DetectorConfig::default(),
        price_stream,
        dex_adapters_for_detector,
        risk_manager,
        executor,
        detector_shutdown_rx,
    );
    let detector_handle = detector.spawn();

    let (mdi_shutdown_tx, mdi_shutdown_rx) = oneshot::channel();
    let mut mdi = MarketDataIngestorProcessor::new(mdi_config, adapters).await?;
    mdi.set_update_sender(update_tx);
    mdi.set_shutdown_receiver(mdi_shutdown_rx);
    let mdi_handle = tokio::spawn(async move { mdi.run_processor().await });

    tokio::signal::ctrl_c().await?;

    // Graceful shutdown
    detector_shutdown_tx.send(()).await.ok();
    mdi_shutdown_tx.send(()).ok();

    if let Err(e) = detector_handle.await.expect("detector task panicked") {
        error!(error = %e, "Detector exited with error");
    }
    if let Err(e) = mdi_handle.await.expect("MDI task panicked") {
        error!(error = %e, "MDI exited with error");
    }

    Ok(())
}
