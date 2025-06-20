use anyhow::Result;
use clap::Parser;
use core::{BotConfig, DummyRiskManager};
use detector::service::DexAdapters;
use detector::traits::{IsExecutor, IsRiskManager};
use detector::{bellman_ford::DetectorConfig, Detector};
use executor::{BlockchainClient, TradeExecutor};
use market_data_ingestor::{IndexerProcessorConfig, MarketDataIngestorProcessor};
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
    /// Path to the market data ingestor configuration YAML
    #[arg(long, default_value = "config/market-data-ingestor.yml")]
    mdi_config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let args = Args::parse();

    // Load main bot configuration
    let bot_cfg = BotConfig::load(&args.config)?;
    bot_cfg.validate()?;

    // Load market-data ingestor configuration
    let mdi_cfg = IndexerProcessorConfig::load(args.mdi_config.into())?;

    // Channel between the ingestor and the detector
    let (update_tx, update_rx) = mpsc::channel(100);
    let price_stream = Box::pin(ReceiverStream::new(update_rx));

    // Instantiate core services
    let risk_manager: Arc<dyn IsRiskManager> = Arc::new(DummyRiskManager::new());
    let executor: Arc<dyn IsExecutor> = Arc::new(
        TradeExecutor::<dex_adapter_trait::Exchange>::new(BlockchainClient::new(None)),
    );
    let dex_adapters: DexAdapters = DexAdapters::new();

    let (detector_shutdown_tx, detector_shutdown_rx) = mpsc::channel(1);
    let detector = Detector::new(
        DetectorConfig::default(),
        price_stream,
        dex_adapters,
        risk_manager,
        executor,
        detector_shutdown_rx,
    );
    let detector_handle = detector.spawn();

    let (mdi_shutdown_tx, mdi_shutdown_rx) = oneshot::channel();
    let mut mdi = MarketDataIngestorProcessor::new(mdi_cfg).await?;
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
