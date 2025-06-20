use anyhow::Result;
use aptos_indexer_processor_sdk::server_framework::ServerArgs;
use clap::Parser;
use market_data_ingestor::{ingestor_config::IndexerProcessorConfig, MarketDataIngestorProcessor};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap();
    let args = ServerArgs::parse();
    let config_from_path = config_lib::load_config_from_path(
        args.config_path.to_str().expect("Path is not valid UTF-8"),
    )
    .await
    .expect("Failed to load config");
    let config = IndexerProcessorConfig::new(
        config_from_path.transaction_stream_config,
        config_from_path.market_data_config,
    );

    // Create a channel for market updates
    let (tx, mut rx) = mpsc::channel(100);

    // Spawn a task to handle received market updates (placeholder)
    let detector_handle = tokio::spawn(async move {
        tracing::info!("Detector mock receiver started");
        while let Some(update) = rx.recv().await {
            tracing::info!(?update, "Received market update for detector");
        }
        tracing::info!("Detector mock receiver finished");
    });

    let mut processor = MarketDataIngestorProcessor::new(config, Default::default()).await?;
    processor.set_update_sender(tx);

    // Run the processor and wait for it and the detector task to complete
    tokio::select! {
        res = processor.run_processor() => {
            if let Err(e) = res {
                tracing::error!(error = %e, "Processor finished with an error");
            } else {
                tracing::info!("Processor finished gracefully");
            }
        },
        _ = detector_handle => {
            tracing::info!("Detector handle finished");
        },
    }
    Ok(())
}
