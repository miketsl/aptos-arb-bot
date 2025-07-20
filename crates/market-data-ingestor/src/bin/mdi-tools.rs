use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use bytes::Buf;
use clap::{Parser, Subcommand};
use prost::Message;
use serde_json::to_string_pretty;
use tracing::{error, info, warn};

use market_data_ingestor::data_source::RecordedBatch;

/// mdi-tools: Unified command-line interface for Market Data Ingestor operations
#[derive(Parser)]
#[command(name = "mdi-tools")]
#[command(about = "Professional unified tooling for Market Data Ingestor operations")]
#[command(version = "2.0.0")]
pub struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Record live blockchain data to protobuf files
    Record {
        /// Path to the main YAML config file
        #[clap(long)]
        config: PathBuf,
        /// Path to recording configuration file (optional)
        #[clap(long)]
        recording_config: Option<PathBuf>,
        /// Output file pattern
        #[clap(long, default_value = "recording_{timestamp}.pb")]
        output: String,
        /// Maximum number of batches to record
        #[clap(long)]
        max_batches: Option<u64>,
        /// Maximum recording duration in seconds
        #[clap(long)]
        max_duration_seconds: Option<u64>,
        /// Enable verbose logging
        #[clap(long, short)]
        verbose: bool,
    },
    /// Convert between protobuf and JSON formats
    Convert {
        /// Input file path
        #[clap(long)]
        input: PathBuf,
        /// Output file path
        #[clap(long)]
        output: PathBuf,
        /// Target format (json, protobuf)
        #[clap(long)]
        format: String,
        /// Enable pretty printing for JSON
        #[clap(long)]
        pretty: bool,
        /// Include metadata in output
        #[clap(long)]
        metadata: bool,
    },
    /// Validate data integrity
    Validate {
        /// Input file path
        #[clap(long)]
        input: PathBuf,
        /// File format (auto-detect if not specified)
        #[clap(long)]
        format: Option<String>,
        /// Detailed validation report
        #[clap(long)]
        detailed: bool,
    },
    /// Display file information and statistics
    Info {
        /// Input file path
        #[clap(long)]
        input: PathBuf,
        /// Show detailed information
        #[clap(long)]
        detailed: bool,
        /// Show pool state information
        #[clap(long)]
        pools: bool,
        /// Show transaction statistics
        #[clap(long)]
        transactions: bool,
        /// Show DEX breakdown
        #[clap(long)]
        dex_stats: bool,
    },
    /// Merge multiple recording files
    Merge {
        /// Input files to merge
        #[clap(long, value_delimiter = ',')]
        inputs: Vec<PathBuf>,
        /// Output file path
        #[clap(long)]
        output: PathBuf,
        /// Sort by timestamp
        #[clap(long)]
        sort: bool,
        /// Remove duplicates
        #[clap(long)]
        deduplicate: bool,
    },
    /// Extract specific data from recordings
    Extract {
        /// Input file path
        #[clap(long)]
        input: PathBuf,
        /// Output file path
        #[clap(long)]
        output: PathBuf,
        /// Time range start (timestamp in seconds)
        #[clap(long)]
        start_time: Option<i64>,
        /// Time range end (timestamp in seconds)
        #[clap(long)]
        end_time: Option<i64>,
        /// Extract specific DEXes (comma-separated)
        #[clap(long)]
        dex_filter: Option<String>,
        /// Extract specific pools (comma-separated)
        #[clap(long)]
        pool_filter: Option<String>,
        /// Extract specific transaction types
        #[clap(long)]
        _tx_type_filter: Option<String>,
    },
    /// Benchmark file processing performance
    Benchmark {
        /// Input file path
        #[clap(long)]
        input: PathBuf,
        /// Number of iterations
        #[clap(long, default_value = "10")]
        iterations: u32,
        /// Benchmark operation (read, parse, convert)
        #[clap(long, default_value = "parse")]
        operation: String,
        /// Show detailed timing breakdown
        #[clap(long)]
        detailed: bool,
    },
}

/// File information structure
#[derive(Debug)]
pub struct FileInfo {
    pub file_size: u64,
    pub batch_count: usize,
    pub transaction_count: usize,
    pub pool_state_count: usize,
    pub time_range: Option<(i64, i64)>,
    pub dex_breakdown: HashMap<String, usize>,
    pub pool_breakdown: HashMap<String, usize>,
    pub version_range: Option<(u64, u64)>,
}

impl FileInfo {
    pub fn format_summary(&self, detailed: bool) -> String {
        let mut output = format!(
            "File Information:\n\
             File Size: {:.2} MB\n\
             Batches: {}\n\
             Transactions: {}\n\
             Pool States: {}",
            self.file_size as f64 / 1024.0 / 1024.0,
            self.batch_count,
            self.transaction_count,
            self.pool_state_count
        );

        if let Some((start, end)) = self.time_range {
            output.push_str(&format!(
                "\nTime Range: {} to {} ({} seconds)",
                chrono::DateTime::from_timestamp(start / 1000, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Invalid".to_string()),
                chrono::DateTime::from_timestamp(end / 1000, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Invalid".to_string()),
                (end - start) / 1000
            ));
        }

        if let Some((start_version, end_version)) = self.version_range {
            output.push_str(&format!(
                "\nVersion Range: {} to {} ({} versions)",
                start_version,
                end_version,
                end_version - start_version + 1
            ));
        }

        if detailed {
            if !self.dex_breakdown.is_empty() {
                output.push_str("\n\nDEX Breakdown:");
                for (dex, count) in &self.dex_breakdown {
                    output.push_str(&format!("\n  {}: {} pools", dex, count));
                }
            }

            if !self.pool_breakdown.is_empty() && self.pool_breakdown.len() <= 20 {
                output.push_str("\n\nTop Pools:");
                let mut pools: Vec<_> = self.pool_breakdown.iter().collect();
                pools.sort_by(|a, b| b.1.cmp(a.1));
                for (pool, count) in pools.iter().take(10) {
                    output.push_str(&format!("\n  {}: {} occurrences", pool, count));
                }
            }
        }

        output
    }
}

/// Benchmark results
#[derive(Debug)]
pub struct BenchmarkResults {
    pub operation: String,
    pub iterations: u32,
    pub total_time_ms: u64,
    pub avg_time_ms: f64,
    pub min_time_ms: u64,
    pub max_time_ms: u64,
    pub throughput_mb_per_sec: f64,
    pub file_size_mb: f64,
}

impl BenchmarkResults {
    pub fn format_summary(&self) -> String {
        format!(
            "Benchmark Results ({}): \n\
             Iterations: {}\n\
             Total Time: {:.2}s\n\
             Average Time: {:.2}ms\n\
             Min Time: {}ms\n\
             Max Time: {}ms\n\
             Throughput: {:.2} MB/s\n\
             File Size: {:.2} MB",
            self.operation,
            self.iterations,
            self.total_time_ms as f64 / 1000.0,
            self.avg_time_ms,
            self.min_time_ms,
            self.max_time_ms,
            self.throughput_mb_per_sec,
            self.file_size_mb
        )
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    match args.command {
        Commands::Record {
            config,
            recording_config,
            output,
            max_batches,
            max_duration_seconds,
            verbose,
        } => record_data(
            config,
            recording_config,
            output,
            max_batches,
            max_duration_seconds,
            verbose,
        ),
        Commands::Convert {
            input,
            output,
            format,
            pretty,
            metadata,
        } => convert_data(input, output, format, pretty, metadata),
        Commands::Validate {
            input,
            format,
            detailed,
        } => validate_data(input, format, detailed),
        Commands::Info {
            input,
            detailed,
            pools,
            transactions,
            dex_stats,
        } => show_file_info(input, detailed, pools, transactions, dex_stats),
        Commands::Merge {
            inputs,
            output,
            sort,
            deduplicate,
        } => merge_files(inputs, output, sort, deduplicate),
        Commands::Extract {
            input,
            output,
            start_time,
            end_time,
            dex_filter,
            pool_filter,
            _tx_type_filter,
        } => extract_data(
            input,
            output,
            start_time,
            end_time,
            dex_filter,
            pool_filter,
            _tx_type_filter,
        ),
        Commands::Benchmark {
            input,
            iterations,
            operation,
            detailed,
        } => benchmark_performance(input, iterations, operation, detailed),
    }
}

fn record_data(
    config: PathBuf,
    recording_config: Option<PathBuf>,
    output: String,
    max_batches: Option<u64>,
    max_duration_seconds: Option<u64>,
    verbose: bool,
) -> Result<()> {
    info!("Starting recording operation");

    // Build command for mdi-recorder
    let mut cmd = std::process::Command::new("mdi-recorder");
    cmd.arg("--config-path").arg(&config);
    cmd.arg("--output").arg(&output);

    if let Some(recording_config) = recording_config {
        cmd.arg("--recording-config").arg(recording_config);
    }

    if let Some(max_batches) = max_batches {
        cmd.arg("--max-batches").arg(max_batches.to_string());
    }

    if let Some(max_duration) = max_duration_seconds {
        cmd.arg("--max-duration-seconds")
            .arg(max_duration.to_string());
    }

    if verbose {
        cmd.arg("--verbose");
    }

    let status = cmd.status().context("Failed to execute mdi-recorder")?;

    if !status.success() {
        return Err(anyhow::anyhow!(
            "Recording failed with exit code: {:?}",
            status.code()
        ));
    }

    info!("Recording completed successfully");
    Ok(())
}

fn convert_data(
    input: PathBuf,
    output: PathBuf,
    format: String,
    pretty: bool,
    metadata: bool,
) -> Result<()> {
    info!("Converting {} to {}", input.display(), output.display());

    match format.to_lowercase().as_str() {
        "json" => convert_to_json(input, output, pretty, metadata),
        "protobuf" | "pb" => convert_to_protobuf(input, output),
        _ => Err(anyhow::anyhow!("Unsupported format: {}", format)),
    }
}

fn convert_to_json(input: PathBuf, output: PathBuf, pretty: bool, metadata: bool) -> Result<()> {
    let data = fs::read(&input)?;
    let mut buf = bytes::BytesMut::from(&data[..]);
    let mut batches = Vec::new();

    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
        batches.push(batch);
    }

    let json_output = if metadata {
        serde_json::json!({
            "metadata": {
                "source_file": input.file_name().unwrap().to_str().unwrap(),
                "conversion_time": chrono::Utc::now().to_rfc3339(),
                "batch_count": batches.len(),
            },
            "batches": batches
        })
    } else {
        serde_json::json!(batches)
    };

    let json_string = if pretty {
        to_string_pretty(&json_output)?
    } else {
        serde_json::to_string(&json_output)?
    };

    fs::write(&output, json_string)?;
    info!("Converted to JSON: {}", output.display());
    Ok(())
}

fn convert_to_protobuf(input: PathBuf, output: PathBuf) -> Result<()> {
    let content = fs::read_to_string(&input)?;
    let json_data: serde_json::Value = serde_json::from_str(&content)?;

    let batches: Vec<RecordedBatch> = if json_data.get("batches").is_some() {
        serde_json::from_value(json_data["batches"].clone())?
    } else {
        serde_json::from_value(json_data)?
    };

    let mut output_data = Vec::new();
    for batch in batches {
        let encoded = batch.encode_length_delimited_to_vec();
        output_data.extend(encoded);
    }

    fs::write(&output, output_data)?;
    info!("Converted to protobuf: {}", output.display());
    Ok(())
}

fn validate_data(input: PathBuf, format: Option<String>, detailed: bool) -> Result<()> {
    info!("Validating file: {}", input.display());

    let format = format.unwrap_or_else(|| {
        input
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string())
    });

    match format.as_str() {
        "pb" => validate_protobuf(input, detailed),
        "json" => validate_json(input, detailed),
        _ => Err(anyhow::anyhow!(
            "Unsupported format for validation: {}",
            format
        )),
    }
}

fn validate_protobuf(input: PathBuf, detailed: bool) -> Result<()> {
    let data = fs::read(&input)?;
    let mut buf = bytes::BytesMut::from(&data[..]);
    let mut batch_count = 0;
    let mut errors = 0;

    while buf.has_remaining() {
        match RecordedBatch::decode_length_delimited(&mut buf) {
            Ok(batch) => {
                batch_count += 1;
                if detailed {
                    // Validate batch contents
                    if batch.start_version > batch.end_version {
                        warn!("Batch {}: Invalid version range", batch_count);
                        errors += 1;
                    }
                    if batch.transactions.is_empty() {
                        warn!("Batch {}: No transactions", batch_count);
                    }
                }
            }
            Err(e) => {
                error!("Failed to decode batch {}: {}", batch_count + 1, e);
                errors += 1;
            }
        }
    }

    if errors == 0 {
        info!("Validation successful: {} batches processed", batch_count);
    } else {
        warn!(
            "Validation completed with {} errors in {} batches",
            errors, batch_count
        );
    }

    Ok(())
}

fn validate_json(input: PathBuf, detailed: bool) -> Result<()> {
    let content = fs::read_to_string(&input)?;

    match serde_json::from_str::<Vec<RecordedBatch>>(&content) {
        Ok(batches) => {
            info!("JSON validation successful: {} batches", batches.len());
            if detailed {
                for (i, batch) in batches.iter().enumerate() {
                    if batch.start_version > batch.end_version {
                        warn!("Batch {}: Invalid version range", i);
                    }
                }
            }
        }
        Err(e) => {
            // Try parsing as metadata format
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(value) => {
                    if value.get("batches").is_some() {
                        let batches: Vec<RecordedBatch> =
                            serde_json::from_value(value["batches"].clone())?;
                        info!(
                            "JSON validation successful (with metadata): {} batches",
                            batches.len()
                        );
                    } else {
                        return Err(anyhow::anyhow!("Invalid JSON structure: {}", e));
                    }
                }
                Err(_) => return Err(anyhow::anyhow!("JSON parsing failed: {}", e)),
            }
        }
    }

    Ok(())
}

fn show_file_info(
    input: PathBuf,
    detailed: bool,
    show_pools: bool,
    show_transactions: bool,
    show_dex_stats: bool,
) -> Result<()> {
    info!("Analyzing file: {}", input.display());

    let file_size = fs::metadata(&input)?.len();
    let data = fs::read(&input)?;
    let mut buf = bytes::BytesMut::from(&data[..]);

    let mut info = FileInfo {
        file_size,
        batch_count: 0,
        transaction_count: 0,
        pool_state_count: 0,
        time_range: None,
        dex_breakdown: HashMap::new(),
        pool_breakdown: HashMap::new(),
        version_range: None,
    };

    let mut min_time = i64::MAX;
    let mut max_time = i64::MIN;
    let mut min_version = u64::MAX;
    let mut max_version = u64::MIN;

    while buf.has_remaining() {
        match RecordedBatch::decode_length_delimited(&mut buf) {
            Ok(batch) => {
                info.batch_count += 1;
                info.transaction_count += batch.transactions.len();
                info.pool_state_count += batch.pool_initializations.len();

                // Track time range
                if batch.timestamp_ms < min_time {
                    min_time = batch.timestamp_ms;
                }
                if batch.timestamp_ms > max_time {
                    max_time = batch.timestamp_ms;
                }

                // Track version range
                if batch.start_version < min_version {
                    min_version = batch.start_version;
                }
                if batch.end_version > max_version {
                    max_version = batch.end_version;
                }

                // Track DEX and pool breakdown
                for pool_state in &batch.pool_initializations {
                    *info
                        .dex_breakdown
                        .entry(pool_state.dex_name.clone())
                        .or_insert(0) += 1;
                    *info
                        .pool_breakdown
                        .entry(pool_state.pool_id.clone())
                        .or_insert(0) += 1;
                }
            }
            Err(e) => {
                error!("Failed to decode batch: {}", e);
            }
        }
    }

    if min_time != i64::MAX {
        info.time_range = Some((min_time, max_time));
    }
    if min_version != u64::MAX {
        info.version_range = Some((min_version, max_version));
    }

    println!("{}", info.format_summary(detailed || show_dex_stats));

    if show_pools && !info.pool_breakdown.is_empty() {
        println!("\nPool Details:");
        let mut pools: Vec<_> = info.pool_breakdown.iter().collect();
        pools.sort_by(|a, b| b.1.cmp(a.1));
        for (pool, count) in pools.iter().take(50) {
            println!("  {}: {} occurrences", pool, count);
        }
    }

    if show_transactions {
        println!("\nTransaction Statistics:");
        println!(
            "  Average transactions per batch: {:.2}",
            info.transaction_count as f64 / info.batch_count as f64
        );
    }

    Ok(())
}

fn merge_files(inputs: Vec<PathBuf>, output: PathBuf, sort: bool, deduplicate: bool) -> Result<()> {
    info!("Merging {} files into {}", inputs.len(), output.display());

    let mut all_batches = Vec::new();

    for input_file in &inputs {
        info!("Reading file: {}", input_file.display());
        let data = fs::read(input_file)?;
        let mut buf = bytes::BytesMut::from(&data[..]);

        while buf.has_remaining() {
            let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
            all_batches.push(batch);
        }
    }

    info!("Loaded {} batches total", all_batches.len());

    if sort {
        info!("Sorting batches by timestamp");
        all_batches.sort_by_key(|batch| batch.timestamp_ms);
    }

    if deduplicate {
        info!("Removing duplicate batches");
        all_batches.dedup_by_key(|batch| (batch.start_version, batch.end_version));
        info!("After deduplication: {} batches", all_batches.len());
    }

    // Write merged output
    let mut output_data = Vec::new();
    for batch in all_batches {
        let encoded = batch.encode_length_delimited_to_vec();
        output_data.extend(encoded);
    }

    fs::write(&output, output_data)?;
    info!("Merge completed: {}", output.display());
    Ok(())
}

fn extract_data(
    input: PathBuf,
    output: PathBuf,
    start_time: Option<i64>,
    end_time: Option<i64>,
    dex_filter: Option<String>,
    pool_filter: Option<String>,
    _tx_type_filter: Option<String>,
) -> Result<()> {
    info!(
        "Extracting data from {} to {}",
        input.display(),
        output.display()
    );

    let dex_list: Option<Vec<String>> =
        dex_filter.map(|s| s.split(',').map(|s| s.trim().to_string()).collect());
    let pool_list: Option<Vec<String>> =
        pool_filter.map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

    let data = fs::read(&input)?;
    let mut buf = bytes::BytesMut::from(&data[..]);
    let mut extracted_batches = Vec::new();

    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)?;

        // Apply time filter
        if let Some(start) = start_time {
            if batch.timestamp_ms < start * 1000 {
                continue;
            }
        }
        if let Some(end) = end_time {
            if batch.timestamp_ms > end * 1000 {
                continue;
            }
        }

        // Apply DEX filter
        if let Some(ref dex_list) = dex_list {
            let has_matching_dex = batch
                .pool_initializations
                .iter()
                .any(|pool| dex_list.contains(&pool.dex_name));
            if !has_matching_dex && !batch.pool_initializations.is_empty() {
                continue;
            }
        }

        // Apply pool filter
        if let Some(ref pool_list) = pool_list {
            let has_matching_pool = batch
                .pool_initializations
                .iter()
                .any(|pool| pool_list.contains(&pool.pool_id));
            if !has_matching_pool && !batch.pool_initializations.is_empty() {
                continue;
            }
        }

        extracted_batches.push(batch);
    }

    info!("Extracted {} batches", extracted_batches.len());

    // Write extracted data
    let mut output_data = Vec::new();
    for batch in extracted_batches {
        let encoded = batch.encode_length_delimited_to_vec();
        output_data.extend(encoded);
    }

    fs::write(&output, output_data)?;
    info!("Extraction completed: {}", output.display());
    Ok(())
}

fn benchmark_performance(
    input: PathBuf,
    iterations: u32,
    operation: String,
    detailed: bool,
) -> Result<()> {
    info!(
        "Benchmarking {} operation on {}",
        operation,
        input.display()
    );

    let file_size = fs::metadata(&input)?.len();
    let file_size_mb = file_size as f64 / 1024.0 / 1024.0;

    let mut times = Vec::new();

    for i in 0..iterations {
        let start = Instant::now();

        match operation.as_str() {
            "read" => {
                let _ = fs::read(&input)?;
            }
            "parse" => {
                let data = fs::read(&input)?;
                let mut buf = bytes::BytesMut::from(&data[..]);
                let mut count = 0;
                while buf.has_remaining() {
                    let _ = RecordedBatch::decode_length_delimited(&mut buf)?;
                    count += 1;
                }
                if detailed {
                    info!("Iteration {}: parsed {} batches", i + 1, count);
                }
            }
            "convert" => {
                let data = fs::read(&input)?;
                let mut buf = bytes::BytesMut::from(&data[..]);
                let mut batches = Vec::new();
                while buf.has_remaining() {
                    let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
                    batches.push(batch);
                }
                let _ = serde_json::to_string(&batches)?;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown benchmark operation: {}",
                    operation
                ))
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;
        times.push(elapsed);

        if detailed {
            info!("Iteration {}: {}ms", i + 1, elapsed);
        }
    }

    let total_time = times.iter().sum::<u64>();
    let avg_time = total_time as f64 / iterations as f64;
    let min_time = *times.iter().min().unwrap();
    let max_time = *times.iter().max().unwrap();
    let throughput = (file_size_mb * iterations as f64) / (total_time as f64 / 1000.0);

    let results = BenchmarkResults {
        operation,
        iterations,
        total_time_ms: total_time,
        avg_time_ms: avg_time,
        min_time_ms: min_time,
        max_time_ms: max_time,
        throughput_mb_per_sec: throughput,
        file_size_mb,
    };

    println!("{}", results.format_summary());
    Ok(())
}
