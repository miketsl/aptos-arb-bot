use anyhow::Result;
use aptos_indexer_processor_sdk::server_framework::ServerArgs;
use clap::Parser;
use dex_adapters::{DexAdapter, HyperionAdapter, ThalaAdapter};
use market_data_ingestor::{ingestor_config::IndexerProcessorConfig, MarketDataIngestorProcessor};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap();

    println!("Starting Market Data Ingestor...");

    let args = ServerArgs::parse();
    println!("Parsed args, loading config from: {:?}", args.config_path);

    let config_from_path = config_lib::load_config_from_path(
        args.config_path.to_str().expect("Path is not valid UTF-8"),
    )
    .await
    .expect("Failed to load config");

    println!("Config loaded successfully");

    // Ensure ingestor config is present
    let ingestor_config = config_from_path.ingestor
        .expect("Enhanced ingestor configuration is required. Please add 'ingestor' section to your config file.");

    println!("Ingestor config found, creating processor config");

    let config = IndexerProcessorConfig::from_enhanced_config(
        config_from_path.transaction_stream_config,
        ingestor_config.clone(),
    );

    println!("Processor config created");

    // Create a channel for market updates
    let (tx, mut rx) = mpsc::channel(100);

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Spawn a task to handle received market updates (placeholder)
    let detector_handle = tokio::spawn(async move {
        tracing::info!("Detector mock receiver started");
        while let Some(update) = rx.recv().await {
            tracing::info!(?update, "Received market update for detector");
        }
        tracing::info!("Detector mock receiver finished");
    });

    println!("Creating processor...");

    // Create DexAdapters based on config
    let mut adapters: HashMap<String, Arc<dyn DexAdapter>> = HashMap::new();

    for adapter_config in &ingestor_config.adapters {
        if adapter_config.enabled {
            match adapter_config.name.as_str() {
                "Hyperion" => {
                    let adapter = HyperionAdapter::new(vec![adapter_config.module_address.clone()]);
                    adapters.insert(adapter_config.name.clone(), Arc::new(adapter));
                    println!("Added Hyperion adapter");
                }
                "ThalaSwap" => {
                    let adapter = ThalaAdapter::new(vec![adapter_config.module_address.clone()]);
                    adapters.insert(adapter_config.name.clone(), Arc::new(adapter));
                    println!("Added ThalaSwap adapter");
                }
                _ => {
                    println!("Unknown adapter: {}", adapter_config.name);
                }
            }
        }
    }

    let mut processor = MarketDataIngestorProcessor::new(config, adapters).await?;
    println!("Processor created");

    processor.set_update_sender(tx);
    processor.set_shutdown_receiver(shutdown_rx);

    println!("Starting processor...");

    // Handle Ctrl+C gracefully
    let shutdown_handle = tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl+c");
        tracing::info!("Shutdown signal received");
        let _ = shutdown_tx.send(());
    });

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
        _ = shutdown_handle => {
            tracing::info!("Shutdown handle finished");
        },
    }
    Ok(())
}
