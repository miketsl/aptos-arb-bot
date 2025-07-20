use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStream;
use clap::Parser;
use config_lib::load_config_from_path;
use prost::Message;
use tokio::runtime::Runtime;

use market_data_ingestor::data_source::RecordedBatch;
use market_data_ingestor::ingestor_config::IndexerProcessorConfig;
use market_data_ingestor::pool_state_manager::{
    PoolStateManager, DataSourceType, PoolFilterConfig,
};

/// mdi-recorder: capture raw transaction batches to a protobuf file for replay.
#[derive(Parser)]
pub struct Args {
    /// Path to the YAML config file
    #[clap(long)]
    config_path: PathBuf,
    /// Output file for recorded batches (protobuf, length-delimited)
    #[clap(long)]
    output: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Use a tokio runtime for async operations
    let rt = Runtime::new()?;
    rt.block_on(async move {
        // Initialize TLS crypto for gRPC
        rustls::crypto::ring::default_provider()
            .install_default()
            .unwrap();
        // Load config and extract transaction stream settings
        let cfg = load_config_from_path(args.config_path.to_str().unwrap()).await?;

        // Ensure ingestor config is present
        let ingestor_config = cfg.ingestor
            .expect("Enhanced ingestor configuration is required. Please add 'ingestor' section to your config file.");

        let indexer_cfg = IndexerProcessorConfig::from_enhanced_config(
            cfg.transaction_stream_config.clone(),
            ingestor_config,
        );

        // Set up gRPC stream
        let mut stream =
            TransactionStream::new(indexer_cfg.transaction_stream_config.clone()).await?;

        // Set up DEX adapters and event router
        let mut event_router = dex_adapters::EventRouter::new();
        
        // Register adapters from configuration using the centralized factory
        for adapter_config in &indexer_cfg.ingestor_config.adapters {
            match dex_adapters::create_adapter_from_config(&adapter_config.name, adapter_config.module_address.clone()) {
                Ok(adapter) => {
                    let adapter_arc = std::sync::Arc::from(adapter);
                    event_router.register_adapter(adapter_arc);
                    println!("Registered adapter: {} with module address: {}", 
                            adapter_config.name, adapter_config.module_address);
                }
                Err(e) => {
                    eprintln!("Failed to create adapter {}: {}", adapter_config.name, e);
                    eprintln!("Skipping unknown adapter. Available adapters: hyperion, thalaswap, tapp");
                }
            }
        }
        
        let event_router = std::sync::Arc::new(event_router);

        // Set up pool state manager with live data source
        let filter_config = PoolFilterConfig {
            min_tvl_usd: None, // Disabled - requires price feed integration
            max_tracked_pools: Some(5000), // Reasonable limit for recording
            dex_whitelist: None, // Track all configured DEXes
            token_whitelist: None, // Track all tokens for now
            token_blacklist: None,
            pool_type_whitelist: None,
        };
        
        let pool_manager = PoolStateManager::new(
            event_router.clone(),
            DataSourceType::Live,
            filter_config,
        );

        // Start background worker result processor
        let _worker_processor_handle = pool_manager.start_worker_result_processor().await;

        // Prepare file writer for length-delimited batches
        let file = File::create(&args.output)?;
        let mut writer = BufWriter::new(file);
        
        println!("Starting enhanced recording to {:?}", args.output);
        println!("Pool state detection: ENABLED");
        println!("Registered DEX adapters: {}", event_router.get_adapters().len());

        // Statistics tracking
        let mut batch_count = 0u64;
        let mut total_transactions = 0u64;
        let mut total_pools_discovered = 0u64;
        let start_time = std::time::Instant::now();
        
        loop {
            let batch = stream.get_next_transaction_batch().await?;
            batch_count += 1;
            total_transactions += batch.transactions.len() as u64;
            
            // Process each transaction for pool discovery
            let mut all_discoveries = Vec::new();
            for transaction in &batch.transactions {
                match pool_manager.extract_pool_ids_from_transaction(transaction).await {
                    Ok(discoveries) => {
                        all_discoveries.extend(discoveries);
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to extract pool IDs from transaction {}: {}", 
                                transaction.version, e);
                    }
                }
            }
            
            // Process discovered pools and determine which ones to fetch
            let pools_to_fetch = pool_manager.process_pool_discoveries(all_discoveries).await;
            
            if !pools_to_fetch.is_empty() {
                println!("Discovered {} new pools in batch {}-{}, spawning {} fetch workers", 
                        pools_to_fetch.len(), batch.start_version, batch.end_version, pools_to_fetch.len());
                
                // Spawn async workers to fetch pool states
                pool_manager.spawn_pool_state_workers(pools_to_fetch).await;
            }
            
            // Get any completed pool states from cache
            let cached_pool_states = pool_manager.drain_cached_pool_states().await;
            let mut pool_initializations = Vec::new();
            
            for (pool_id, pool_state) in cached_pool_states {
                // Clone values for logging before moving
                let dex_name = pool_state.dex_name.clone();
                let token_a = pool_state.token_a.clone();
                let token_b = pool_state.token_b.clone();
                
                // Extract pool type and multi-token data from additional_data
                let pool_type = pool_state.additional_data.get("pool_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                
                let all_tokens = pool_state.additional_data.get("all_tokens")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<String>>())
                    .unwrap_or_default();
                
                let all_reserves = pool_state.additional_data.get("all_reserves")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<String>>())
                    .unwrap_or_default();
                
                let all_weights = pool_state.additional_data.get("all_weights")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u32)).collect::<Vec<u32>>())
                    .unwrap_or_default();
                
                // Convert PoolState to RecordedPoolState using hybrid design
                let recorded_pool_state = market_data_ingestor::data_source::RecordedPoolState {
                    pool_id: pool_state.pool_id,
                    dex_name: pool_state.dex_name,
                    // Fast path: Primary pair
                    token_a: pool_state.token_a,
                    token_b: pool_state.token_b,
                    reserve_a: pool_state.reserve_a,
                    reserve_b: pool_state.reserve_b,
                    fee_rate: pool_state.fee_rate,
                    // Complete data: Multi-token support
                    all_tokens,
                    all_reserves,
                    all_weights,
                    // Metadata
                    pool_type,
                    block_height: pool_state.block_height,
                    additional_data: serde_json::to_vec(&pool_state.additional_data)
                        .unwrap_or_else(|_| b"{}".to_vec()),
                };
                
                // Enhanced logging for multi-token pools
                if recorded_pool_state.all_tokens.len() > 2 {
                    println!("Embedded multi-token pool state: {} ({}) - {} tokens: {:?}", 
                            pool_id, dex_name, recorded_pool_state.all_tokens.len(), recorded_pool_state.all_tokens);
                } else {
                    println!("Embedded pool state: {} ({}) - {}/{}", 
                            pool_id, dex_name, token_a, token_b);
                }
                
                pool_initializations.push(recorded_pool_state);
                total_pools_discovered += 1;
            }
            
            let rec = RecordedBatch {
                start_version: batch.start_version,
                end_version: batch.end_version,
                timestamp_ms: batch
                    .start_txn_timestamp
                    .map_or(0, |ts| ts.seconds * 1000 + (ts.nanos as i64) / 1_000_000),
                transactions: batch.transactions,
                pool_initializations,
            };
            
            // Serialize into an in-memory buffer, then write to file
            let mut buf = bytes::BytesMut::new();
            rec.encode_length_delimited(&mut buf)?;
            writer.write_all(&buf)?;
            writer.flush()?;
            
            // Print progress and statistics
            if batch_count % 100 == 0 {
                let stats = pool_manager.get_stats().await;
                let registry_sizes = pool_manager.get_registry_sizes().await;
                let elapsed = start_time.elapsed();
                
                println!("=== Recording Progress ===");
                println!("Batches: {}, Transactions: {}, Pools Discovered: {}", 
                        batch_count, total_transactions, total_pools_discovered);
                println!("Pool Stats - Known: {}, Rejected: {}, Pending: {}, Cached: {}", 
                        registry_sizes.known_pools, registry_sizes.rejected_pools, 
                        registry_sizes.pending_pools, registry_sizes.cached_states);
                println!("API Calls - Made: {}, Success: {}, Failed: {}", 
                        stats.api_calls_made, stats.api_calls_successful, stats.api_calls_failed);
                println!("Elapsed: {:.2}s, Rate: {:.1} batches/s", 
                        elapsed.as_secs_f64(), batch_count as f64 / elapsed.as_secs_f64());
                println!("Current batch: versions {}-{}", rec.start_version, rec.end_version);
                println!("========================");
            }
            
            // Periodic cache cleanup
            if batch_count % 1000 == 0 {
                pool_manager.cleanup_expired_cache().await;
            }
        }
    })
}
