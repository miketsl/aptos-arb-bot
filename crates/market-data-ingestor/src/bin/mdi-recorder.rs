use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStream;
use clap::Parser;
use config_lib::load_config_from_path;
use prost::Message;
use tokio::runtime::Runtime;
use tokio::signal;
use tracing::{info, warn, error};

use market_data_ingestor::data_source::{RecordedBatch, RecordedPoolState};
use market_data_ingestor::file_rotation::FileRotationManager;
use market_data_ingestor::ingestor_config::IndexerProcessorConfig;
use market_data_ingestor::pool_state_manager::{
    PoolStateManager, DataSourceType, PoolFilterConfig,
};
use market_data_ingestor::recording_config::RecordingConfig;
use market_data_ingestor::recording_monitor::{RecordingMonitor, ProgressReporter};

/// mdi-recorder: capture raw transaction batches with pool state embedding and professional monitoring
#[derive(Parser)]
#[command(name = "mdi-recorder")]
#[command(about = "Professional recording tool with pool state management and file rotation")]
#[command(version = "2.0.0")]
pub struct Args {
    /// Path to the main YAML config file (for gRPC connection)
    #[clap(long, required_unless_present = "generate_config")]
    config_path: Option<PathBuf>,

    /// Path to recording configuration file (optional, uses defaults if not provided)
    #[clap(long)]
    recording_config: Option<PathBuf>,

    /// Output file pattern (supports {timestamp} substitution)
    #[clap(long, default_value = "recording_{timestamp}.pb")]
    output: String,

    /// Maximum number of batches to record (0 = unlimited, overrides config)
    #[clap(long)]
    max_batches: Option<u64>,

    /// Maximum recording duration in seconds (0 = unlimited, overrides config)
    #[clap(long)]
    max_duration_seconds: Option<u64>,

    /// Enable verbose logging
    #[clap(long, short)]
    verbose: bool,

    /// Disable pool state detection and embedding
    #[clap(long)]
    no_pool_detection: bool,

    /// Disable file rotation
    #[clap(long)]
    no_rotation: bool,

    /// Generate sample recording configuration file and exit
    #[clap(long)]
    generate_config: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Handle config generation first (before other validation)
    if args.generate_config {
        return generate_sample_config();
    }

    // Initialize logging
    if args.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    // Use a tokio runtime for async operations
    let rt = Runtime::new()?;
    rt.block_on(async move {
        // Initialize TLS crypto for gRPC
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("Failed to install rustls crypto provider");

        // Load configurations
        let config_path = args.config_path.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Config path is required for recording"))?;
        let main_config = load_main_config(config_path).await?;
        let recording_config = load_recording_config(&args)?;

        info!("Starting mdi-recorder v2.0.0");
        info!("Main config: {}", config_path.display());
        if let Some(ref path) = args.recording_config {
            info!("Recording config: {}", path.display());
        }

        // Validate recording configuration
        recording_config.validate()
            .map_err(|e| anyhow::anyhow!("Invalid recording configuration: {}", e))?;

        // Setup file rotation manager
        let mut file_manager = setup_file_manager(&args, &recording_config)?;

        // Setup monitoring
        let monitor = RecordingMonitor::new(recording_config.monitoring.clone());
        let stats_handle = monitor.stats_handle();
        monitor.start_monitoring().await;

        // Setup progress reporter
        let mut progress_reporter = ProgressReporter::new(
            stats_handle.clone(),
            recording_config.monitoring.progress_report_interval,
        );

        // Setup pool state manager if enabled
        let pool_state_manager = if args.no_pool_detection || !recording_config.pool_detection.enabled {
            info!("Pool state detection disabled");
            None
        } else {
            info!("Pool state detection enabled");
            let pool_filter_config = create_pool_filter_config(&recording_config);
            let event_router = setup_event_router(&main_config).await?;
            Some(PoolStateManager::new(event_router, DataSourceType::Live, pool_filter_config))
        };

        // Create transaction stream
        let mut stream = TransactionStream::new(main_config.transaction_stream_config.clone()).await?;

        info!("Recording to: {}", args.output);
        info!("Press Ctrl+C to stop recording...");

        // Setup graceful shutdown
        let shutdown_signal = setup_shutdown_signal();

        // Recording loop
        tokio::select! {
            result = recording_loop(
                &mut stream,
                &mut file_manager,
                &pool_state_manager,
                &stats_handle,
                &mut progress_reporter,
                &monitor,
                &recording_config,
            ) => {
                match result {
                    Ok(_) => info!("Recording completed successfully"),
                    Err(e) => error!("Recording failed: {}", e),
                }
            }
            _ = shutdown_signal => {
                info!("Shutdown signal received, stopping recording...");
            }
        }

        // Final cleanup and summary
        file_manager.flush()?;
        monitor.print_final_summary().await;

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

/// Load main configuration
async fn load_main_config(config_path: &PathBuf) -> anyhow::Result<IndexerProcessorConfig> {
    let cfg = load_config_from_path(config_path.to_str().unwrap()).await?;
    
    let ingestor_config = cfg.ingestor
        .ok_or_else(|| anyhow::anyhow!("Enhanced ingestor configuration is required. Please add 'ingestor' section to your config file."))?;

    Ok(IndexerProcessorConfig::from_enhanced_config(
        cfg.transaction_stream_config,
        ingestor_config,
    ))
}

/// Setup event router with DEX adapters
async fn setup_event_router(config: &IndexerProcessorConfig) -> anyhow::Result<Arc<dex_adapters::EventRouter>> {
    let mut event_router = dex_adapters::EventRouter::new();
    
    // Register adapters from configuration
    for adapter_config in &config.ingestor_config.adapters {
        match dex_adapters::create_adapter_from_config(&adapter_config.name, adapter_config.module_address.clone()) {
            Ok(adapter) => {
                let adapter_arc = Arc::from(adapter);
                event_router.register_adapter(adapter_arc);
                info!("Registered adapter: {} with module address: {}", 
                      adapter_config.name, adapter_config.module_address);
            }
            Err(e) => {
                warn!("Failed to create adapter {}: {}", adapter_config.name, e);
                warn!("Skipping unknown adapter. Available adapters: hyperion, thalaswap, tapp");
            }
        }
    }
    
    Ok(Arc::new(event_router))
}

/// Load recording configuration from file or use defaults
fn load_recording_config(args: &Args) -> anyhow::Result<RecordingConfig> {
    let mut config = if let Some(ref path) = args.recording_config {
        info!("Loading recording config from: {}", path.display());
        let content = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&content)?
    } else {
        info!("Using default recording configuration");
        RecordingConfig::default()
    };

    // Apply CLI overrides
    if let Some(max_batches) = args.max_batches {
        config.recording.max_batches = max_batches;
    }
    if let Some(max_duration) = args.max_duration_seconds {
        config.recording.max_duration_seconds = max_duration;
    }
    if args.no_rotation {
        config.recording.file_rotation.enabled = false;
    }

    Ok(config)
}

/// Setup file rotation manager
fn setup_file_manager(args: &Args, config: &RecordingConfig) -> anyhow::Result<FileRotationManager> {
    let rotation_settings = if args.no_rotation {
        let mut settings = config.recording.file_rotation.clone();
        settings.enabled = false;
        settings
    } else {
        config.recording.file_rotation.clone()
    };

    FileRotationManager::new(&args.output, rotation_settings)
}

/// Create pool filter configuration from recording config
fn create_pool_filter_config(config: &RecordingConfig) -> PoolFilterConfig {
    PoolFilterConfig {
        min_tvl_usd: config.pool_detection.filters.min_tvl_usd,
        token_whitelist: None, // Use main config for token filtering
        token_blacklist: config.pool_detection.filters.token_blacklist.clone(),
        dex_whitelist: config.pool_detection.filters.dex_whitelist.clone(),
        pool_type_whitelist: config.pool_detection.filters.pool_type_whitelist.clone(),
        max_tracked_pools: Some(config.pool_detection.filters.max_tracked_pools as usize),
    }
}

/// Setup graceful shutdown signal handling
async fn setup_shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// Main recording loop
async fn recording_loop(
    stream: &mut TransactionStream,
    file_manager: &mut FileRotationManager,
    pool_state_manager: &Option<PoolStateManager>,
    stats_handle: &Arc<tokio::sync::RwLock<market_data_ingestor::recording_monitor::RecordingStats>>,
    progress_reporter: &mut ProgressReporter,
    monitor: &RecordingMonitor,
    config: &RecordingConfig,
) -> anyhow::Result<()> {
    // Start worker result processor if pool state manager is enabled
    let _worker_handle = if let Some(ref manager) = pool_state_manager {
        Some(manager.start_worker_result_processor().await)
    } else {
        None
    };

    loop {
        // Check if we should continue recording
        if !monitor.should_continue(
            config.recording.max_batches,
            config.recording.max_duration_seconds,
        ).await {
            break;
        }

        // Get next batch
        let batch_start = Instant::now();
        let batch_result = stream.get_next_transaction_batch().await;
        let batch = match batch_result {
            Ok(batch) => batch,
            Err(e) => {
                error!("Error getting batch: {}", e);
                stats_handle.write().await.record_connection_error();
                continue;
            }
        };

        // Process pool discovery for this batch if enabled
        let pool_initializations = if let Some(ref manager) = pool_state_manager {
            match process_batch_for_pool_discovery(manager, &batch).await {
                Ok(states) => {
                    for _state in &states {
                        stats_handle.write().await.record_pool_state_fetched();
                    }
                    states
                }
                Err(e) => {
                    warn!("Pool discovery failed for batch: {}", e);
                    stats_handle.write().await.record_pool_fetch_failure(false);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        // Create recorded batch with embedded pool states
        let recorded_batch = RecordedBatch {
            start_version: batch.start_version,
            end_version: batch.end_version,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            transactions: batch.transactions.clone(),
            pool_initializations,
        };

        // Write to file
        let encoded = recorded_batch.encode_length_delimited_to_vec();
        match file_manager.write(&encoded) {
            Ok(_) => {
                let processing_time = batch_start.elapsed().as_millis() as u64;
                stats_handle.write().await.record_batch_processed(
                    processing_time,
                    batch.transactions.len() as u64,
                    encoded.len() as u64,
                );
            }
            Err(e) => {
                error!("Failed to write batch: {}", e);
                stats_handle.write().await.record_write_error();
                continue;
            }
        }

        // Report progress if needed
        progress_reporter.maybe_report_progress().await;
    }

    Ok(())
}

/// Generate sample recording configuration file
fn generate_sample_config() -> anyhow::Result<()> {
    let sample_config = RecordingConfig::default();
    let yaml_content = serde_yaml::to_string(&sample_config)?;
    
    let config_path = "recording_config_sample.yml";
    std::fs::write(config_path, yaml_content)?;
    
    println!("Generated sample recording configuration: {}", config_path);
    println!("\nTo use this configuration:");
    println!("1. Edit the file to customize settings");
    println!("2. Run: mdi-recorder --config-path main.yml --recording-config {}", config_path);
    
    Ok(())
}

/// Process a batch of transactions to discover new pools and fetch their states
async fn process_batch_for_pool_discovery(
    pool_manager: &PoolStateManager,
    batch: &aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionsPBResponse,
) -> anyhow::Result<Vec<RecordedPoolState>> {
    let mut all_discoveries = Vec::new();
    
    // Extract pool discoveries from each transaction in the batch
    for transaction in &batch.transactions {
        match pool_manager.extract_pool_ids_from_transaction(transaction).await {
            Ok(discoveries) => {
                all_discoveries.extend(discoveries);
            }
            Err(e) => {
                warn!("Failed to extract pool IDs from transaction {}: {}", transaction.version, e);
            }
        }
    }
    
    if all_discoveries.is_empty() {
        return Ok(Vec::new());
    }
    
    info!("Found {} potential pool discoveries in batch", all_discoveries.len());
    
    // Process discoveries to determine which pools to track
    let pools_to_fetch = pool_manager.process_pool_discoveries(all_discoveries).await;
    
    if pools_to_fetch.is_empty() {
        return Ok(Vec::new());
    }
    
    info!("Will fetch state for {} new pools", pools_to_fetch.len());
    
    // Spawn async workers to fetch pool states
    pool_manager.spawn_pool_state_workers(pools_to_fetch).await;
    
    // Get any completed pool states from cache
    let cached_states = pool_manager.drain_cached_pool_states().await;
    
    // Convert PoolState to RecordedPoolState
    let mut recorded_states = Vec::new();
    for (pool_id, pool_state) in cached_states {
        let recorded_state = convert_pool_state_to_recorded(pool_state, batch.start_version);
        recorded_states.push(recorded_state);
        info!("Converted pool state for {} to recorded format", pool_id);
    }
    
    Ok(recorded_states)
}

/// Convert PoolState to RecordedPoolState for embedding in recordings
fn convert_pool_state_to_recorded(pool_state: dex_adapters::PoolState, block_height: u64) -> RecordedPoolState {
    // Extract additional token data if available
    let (all_tokens, all_reserves, all_weights) = if let Some(tokens) = pool_state.additional_data.get("all_tokens") {
        let tokens_vec = tokens.as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        
        let reserves_vec = pool_state.additional_data.get("all_reserves")
            .and_then(|r| r.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        
        let weights_vec = pool_state.additional_data.get("all_weights")
            .and_then(|w| w.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u32)).collect())
            .unwrap_or_default();
        
        (tokens_vec, reserves_vec, weights_vec)
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    
    // Extract pool type
    let pool_type = pool_state.additional_data.get("pool_type")
        .and_then(|pt| pt.as_str())
        .unwrap_or("constant_product")
        .to_string();
    
    // Serialize additional data
    let additional_data = serde_json::to_vec(&pool_state.additional_data)
        .unwrap_or_default();
    
    RecordedPoolState {
        pool_id: pool_state.pool_id,
        dex_name: pool_state.dex_name,
        token_a: pool_state.token_a,
        token_b: pool_state.token_b,
        reserve_a: pool_state.reserve_a,
        reserve_b: pool_state.reserve_b,
        fee_rate: pool_state.fee_rate,
        all_tokens,
        all_reserves,
        all_weights,
        pool_type,
        block_height,
        additional_data,
    }
}