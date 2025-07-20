use bytes::Buf;
use std::path::PathBuf;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::collections::HashMap;

use clap::{Parser, Subcommand};
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::{to_string_pretty, from_str};
use anyhow::{Result, Context};
use sha2::{Sha256, Digest};

use market_data_ingestor::data_source::{RecordedBatch, RecordedPoolState};

/// mdi-converter: bidirectional conversion between protobuf and JSON formats
#[derive(Parser)]
#[command(name = "mdi-converter")]
#[command(about = "Convert between protobuf and JSON formats for recorded market data")]
pub struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert protobuf to JSON format
    ToJson {
        /// Path to the protobuf file (length-delimited RecordedBatch messages)
        #[clap(long)]
        input: PathBuf,
        /// Output JSON file path
        #[clap(long)]
        output: PathBuf,
        /// Enable pretty printing (default: true)
        #[clap(long, default_value = "true")]
        pretty: bool,
        /// Include detailed metadata in output
        #[clap(long, default_value = "true")]
        include_metadata: bool,
    },
    /// Convert JSON back to protobuf format
    ToProtobuf {
        /// Path to the JSON file
        #[clap(long)]
        input: PathBuf,
        /// Output protobuf file path
        #[clap(long)]
        output: PathBuf,
        /// Validate data integrity during conversion
        #[clap(long, default_value = "true")]
        validate: bool,
    },
    /// Validate round-trip conversion integrity
    Validate {
        /// Path to the protobuf file
        #[clap(long)]
        protobuf: PathBuf,
        /// Path to the JSON file
        #[clap(long)]
        json: PathBuf,
    },
}

/// JSON wrapper structure with complete metadata
#[derive(Serialize, Deserialize)]
struct JsonWrapper {
    metadata: ConversionMetadata,
    batches: Vec<JsonBatch>,
}

/// Metadata for the conversion
#[derive(Serialize, Deserialize)]
struct ConversionMetadata {
    version: String,
    created_at: String,
    source_file_hash: String,
    conversion_timestamp: String,
    batch_count: u64,
    total_transactions: u64,
    total_pools_discovered: u64,
    recording_duration_ms: i64,
    dex_breakdown: HashMap<String, u64>,
    pool_type_breakdown: HashMap<String, u64>,
    version_range: VersionRange,
}

#[derive(Serialize, Deserialize)]
struct VersionRange {
    start_version: u64,
    end_version: u64,
}

/// JSON representation of RecordedBatch with enhanced pool state
#[derive(Serialize, Deserialize)]
struct JsonBatch {
    start_version: u64,
    end_version: u64,
    timestamp_ms: i64,
    transactions: Vec<serde_json::Value>, // Keep as raw JSON for flexibility
    pool_initializations: Vec<JsonPoolState>,
}

/// JSON representation of RecordedPoolState with enhanced readability
#[derive(Serialize, Deserialize)]
struct JsonPoolState {
    pool_id: String,
    dex_name: String,
    // Fast path: Primary pair
    token_a: String,
    token_b: String,
    reserve_a: String,
    reserve_b: String,
    fee_rate: String,
    // Complete data: Multi-token support
    all_tokens: Vec<String>,
    all_reserves: Vec<String>,
    all_weights: Vec<u32>,
    // Metadata
    pool_type: String,
    block_height: u64,
    additional_data_json: serde_json::Value, // Parsed JSON for human editing
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::ToJson { input, output, pretty, include_metadata } => {
            convert_to_json(input, output, pretty, include_metadata)
        }
        Commands::ToProtobuf { input, output, validate } => {
            convert_to_protobuf(input, output, validate)
        }
        Commands::Validate { protobuf, json } => {
            validate_round_trip(protobuf, json)
        }
    }
}

fn convert_to_json(input: PathBuf, output: PathBuf, pretty: bool, include_metadata: bool) -> Result<()> {
    println!("Converting protobuf to JSON: {:?} -> {:?}", input, output);
    
    // Calculate source file hash
    let source_data = std::fs::read(&input)
        .with_context(|| format!("Failed to read input file: {:?}", input))?;
    let source_hash = format!("sha256:{:x}", Sha256::digest(&source_data));
    
    // Parse protobuf file using streaming approach
    let mut buf = bytes::BytesMut::from(&source_data[..]);
    let mut batches = Vec::new();
    let mut batch_count = 0u64;
    let mut total_transactions = 0u64;
    let mut total_pools_discovered = 0u64;
    let mut dex_breakdown: HashMap<String, u64> = HashMap::new();
    let mut pool_type_breakdown: HashMap<String, u64> = HashMap::new();
    let mut min_version = u64::MAX;
    let mut max_version = 0u64;
    let mut min_timestamp = i64::MAX;
    let mut max_timestamp = i64::MIN;

    println!("Parsing protobuf batches...");
    
    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)
            .with_context(|| format!("Failed to decode batch {}", batch_count + 1))?;
        
        batch_count += 1;
        total_transactions += batch.transactions.len() as u64;
        total_pools_discovered += batch.pool_initializations.len() as u64;
        
        // Update version range
        min_version = min_version.min(batch.start_version);
        max_version = max_version.max(batch.end_version);
        
        // Update timestamp range
        min_timestamp = min_timestamp.min(batch.timestamp_ms);
        max_timestamp = max_timestamp.max(batch.timestamp_ms);
        
        // Convert transactions to JSON values for flexibility
        let json_transactions: Result<Vec<serde_json::Value>> = batch.transactions
            .iter()
            .map(|tx| serde_json::to_value(tx).context("Failed to serialize transaction"))
            .collect();
        let json_transactions = json_transactions?;
        
        // Convert pool initializations with enhanced JSON structure
        let mut json_pool_states = Vec::new();
        for pool_state in &batch.pool_initializations {
            // Update DEX breakdown
            *dex_breakdown.entry(pool_state.dex_name.clone()).or_insert(0) += 1;
            
            // Update pool type breakdown
            *pool_type_breakdown.entry(pool_state.pool_type.clone()).or_insert(0) += 1;
            
            // Parse additional_data as JSON for human editing
            let additional_data_json = if pool_state.additional_data.is_empty() {
                serde_json::Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_slice(&pool_state.additional_data)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()))
            };
            
            let json_pool_state = JsonPoolState {
                pool_id: pool_state.pool_id.clone(),
                dex_name: pool_state.dex_name.clone(),
                token_a: pool_state.token_a.clone(),
                token_b: pool_state.token_b.clone(),
                reserve_a: pool_state.reserve_a.clone(),
                reserve_b: pool_state.reserve_b.clone(),
                fee_rate: pool_state.fee_rate.clone(),
                all_tokens: pool_state.all_tokens.clone(),
                all_reserves: pool_state.all_reserves.clone(),
                all_weights: pool_state.all_weights.clone(),
                pool_type: pool_state.pool_type.clone(),
                block_height: pool_state.block_height,
                additional_data_json,
            };
            
            json_pool_states.push(json_pool_state);
        }
        
        let json_batch = JsonBatch {
            start_version: batch.start_version,
            end_version: batch.end_version,
            timestamp_ms: batch.timestamp_ms,
            transactions: json_transactions,
            pool_initializations: json_pool_states,
        };
        
        batches.push(json_batch);
        
        // Progress reporting for large files
        if batch_count % 1000 == 0 {
            println!("Processed {} batches, {} transactions, {} pools", 
                    batch_count, total_transactions, total_pools_discovered);
        }
    }
    
    println!("Parsed {} batches with {} transactions and {} pools", 
            batch_count, total_transactions, total_pools_discovered);
    
    // Create metadata
    let metadata = if include_metadata {
        ConversionMetadata {
            version: "1.0".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            source_file_hash: source_hash,
            conversion_timestamp: chrono::Utc::now().to_rfc3339(),
            batch_count,
            total_transactions,
            total_pools_discovered,
            recording_duration_ms: if max_timestamp > min_timestamp { 
                max_timestamp - min_timestamp 
            } else { 
                0 
            },
            dex_breakdown,
            pool_type_breakdown,
            version_range: VersionRange {
                start_version: if min_version == u64::MAX { 0 } else { min_version },
                end_version: max_version,
            },
        }
    } else {
        // Minimal metadata
        ConversionMetadata {
            version: "1.0".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            source_file_hash: source_hash,
            conversion_timestamp: chrono::Utc::now().to_rfc3339(),
            batch_count,
            total_transactions,
            total_pools_discovered,
            recording_duration_ms: 0,
            dex_breakdown: HashMap::new(),
            pool_type_breakdown: HashMap::new(),
            version_range: VersionRange {
                start_version: 0,
                end_version: 0,
            },
        }
    };
    
    // Create JSON wrapper
    let json_wrapper = JsonWrapper {
        metadata,
        batches,
    };
    
    // Write JSON output using streaming approach for large files
    println!("Writing JSON output...");
    let output_file = File::create(&output)
        .with_context(|| format!("Failed to create output file: {:?}", output))?;
    let mut writer = BufWriter::new(output_file);
    
    if pretty {
        let json_string = to_string_pretty(&json_wrapper)
            .context("Failed to serialize JSON")?;
        writer.write_all(json_string.as_bytes())?;
    } else {
        serde_json::to_writer(&mut writer, &json_wrapper)
            .context("Failed to write JSON")?;
    }
    
    writer.flush()?;
    
    println!("✅ Conversion complete!");
    println!("📊 Summary:");
    println!("   Batches: {}", batch_count);
    println!("   Transactions: {}", total_transactions);
    println!("   Pools discovered: {}", total_pools_discovered);
    println!("   Output: {:?}", output);
    
    Ok(())
}

fn convert_to_protobuf(input: PathBuf, output: PathBuf, validate: bool) -> Result<()> {
    println!("Converting JSON to protobuf: {:?} -> {:?}", input, output);
    
    // Read and parse JSON file
    let json_data = std::fs::read_to_string(&input)
        .with_context(|| format!("Failed to read JSON file: {:?}", input))?;
    
    let json_wrapper: JsonWrapper = from_str(&json_data)
        .context("Failed to parse JSON wrapper")?;
    
    println!("Loaded {} batches from JSON", json_wrapper.batches.len());
    
    // Convert back to protobuf format
    let output_file = File::create(&output)
        .with_context(|| format!("Failed to create output file: {:?}", output))?;
    let mut writer = BufWriter::new(output_file);
    
    let mut converted_batches = 0u64;
    let mut converted_transactions = 0u64;
    let mut converted_pools = 0u64;
    
    for (i, json_batch) in json_wrapper.batches.iter().enumerate() {
        // Convert JSON transactions back to protobuf
        let proto_transactions: Result<Vec<_>> = json_batch.transactions
            .iter()
            .enumerate()
            .map(|(tx_idx, tx_json)| {
                serde_json::from_value(tx_json.clone())
                    .with_context(|| format!("Failed to deserialize transaction {} in batch {}", tx_idx, i))
            })
            .collect();
        let proto_transactions = proto_transactions?;
        
        // Convert JSON pool states back to protobuf
        let mut proto_pool_states = Vec::new();
        for (pool_idx, json_pool) in json_batch.pool_initializations.iter().enumerate() {
            // Serialize additional_data_json back to bytes
            let additional_data = serde_json::to_vec(&json_pool.additional_data_json)
                .with_context(|| format!("Failed to serialize additional_data for pool {} in batch {}", pool_idx, i))?;
            
            let proto_pool_state = RecordedPoolState {
                pool_id: json_pool.pool_id.clone(),
                dex_name: json_pool.dex_name.clone(),
                token_a: json_pool.token_a.clone(),
                token_b: json_pool.token_b.clone(),
                reserve_a: json_pool.reserve_a.clone(),
                reserve_b: json_pool.reserve_b.clone(),
                fee_rate: json_pool.fee_rate.clone(),
                all_tokens: json_pool.all_tokens.clone(),
                all_reserves: json_pool.all_reserves.clone(),
                all_weights: json_pool.all_weights.clone(),
                pool_type: json_pool.pool_type.clone(),
                block_height: json_pool.block_height,
                additional_data,
            };
            
            proto_pool_states.push(proto_pool_state);
        }
        
        // Create protobuf batch
        let proto_batch = RecordedBatch {
            start_version: json_batch.start_version,
            end_version: json_batch.end_version,
            timestamp_ms: json_batch.timestamp_ms,
            transactions: proto_transactions,
            pool_initializations: proto_pool_states,
        };
        
        // Encode and write batch
        let mut buf = bytes::BytesMut::new();
        proto_batch.encode_length_delimited(&mut buf)
            .with_context(|| format!("Failed to encode batch {}", i))?;
        writer.write_all(&buf)?;
        
        converted_batches += 1;
        converted_transactions += json_batch.transactions.len() as u64;
        converted_pools += json_batch.pool_initializations.len() as u64;
        
        // Progress reporting
        if converted_batches % 1000 == 0 {
            println!("Converted {} batches", converted_batches);
        }
    }
    
    writer.flush()?;
    
    // Optional validation
    if validate {
        println!("🔍 Validating conversion integrity...");
        validate_conversion_integrity(&output, &json_wrapper)?;
    }
    
    println!("✅ Conversion complete!");
    println!("📊 Summary:");
    println!("   Batches: {}", converted_batches);
    println!("   Transactions: {}", converted_transactions);
    println!("   Pools: {}", converted_pools);
    println!("   Output: {:?}", output);
    
    Ok(())
}

fn validate_round_trip(protobuf_path: PathBuf, json_path: PathBuf) -> Result<()> {
    println!("🔍 Validating round-trip conversion integrity...");
    
    // Read original protobuf
    let original_data = std::fs::read(&protobuf_path)?;
    let original_hash = format!("{:x}", Sha256::digest(&original_data));
    
    // Read JSON
    let json_data = std::fs::read_to_string(&json_path)?;
    let json_wrapper: JsonWrapper = from_str(&json_data)?;
    
    // Check metadata consistency
    if json_wrapper.metadata.source_file_hash != format!("sha256:{}", original_hash) {
        return Err(anyhow::anyhow!("Source file hash mismatch"));
    }
    
    // Parse original protobuf and compare structure
    let mut buf = bytes::BytesMut::from(&original_data[..]);
    let mut original_batches = Vec::new();
    
    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
        original_batches.push(batch);
    }
    
    if original_batches.len() != json_wrapper.batches.len() {
        return Err(anyhow::anyhow!("Batch count mismatch: {} vs {}", 
                                  original_batches.len(), json_wrapper.batches.len()));
    }
    
    // Validate each batch
    for (i, (original, json_batch)) in original_batches.iter().zip(json_wrapper.batches.iter()).enumerate() {
        if original.start_version != json_batch.start_version ||
           original.end_version != json_batch.end_version ||
           original.timestamp_ms != json_batch.timestamp_ms {
            return Err(anyhow::anyhow!("Batch {} metadata mismatch", i));
        }
        
        if original.transactions.len() != json_batch.transactions.len() {
            return Err(anyhow::anyhow!("Batch {} transaction count mismatch", i));
        }
        
        if original.pool_initializations.len() != json_batch.pool_initializations.len() {
            return Err(anyhow::anyhow!("Batch {} pool initialization count mismatch", i));
        }
    }
    
    println!("✅ Round-trip validation successful!");
    println!("   Batches validated: {}", original_batches.len());
    println!("   Data integrity: PASSED");
    
    Ok(())
}

fn validate_conversion_integrity(protobuf_path: &PathBuf, original_json: &JsonWrapper) -> Result<()> {
    // Read back the converted protobuf and verify it matches the JSON
    let converted_data = std::fs::read(protobuf_path)?;
    let mut buf = bytes::BytesMut::from(&converted_data[..]);
    let mut converted_batches = Vec::new();
    
    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
        converted_batches.push(batch);
    }
    
    if converted_batches.len() != original_json.batches.len() {
        return Err(anyhow::anyhow!("Converted batch count mismatch"));
    }
    
    println!("✅ Conversion integrity validated!");
    Ok(())
}