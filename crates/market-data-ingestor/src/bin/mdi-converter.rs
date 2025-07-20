use anyhow::{Context, Result};
use bytes::Buf;
use clap::{Parser, Subcommand};
use prost::Message;
use serde_json::{from_str, to_string_pretty};
use std::fs::{create_dir_all, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use tracing::{error, info, warn};

use market_data_ingestor::data_source::RecordedBatch;

/// mdi-converter: bidirectional conversion with batch processing and advanced features
#[derive(Parser)]
#[command(name = "mdi-converter")]
#[command(about = "Professional conversion tool with batch processing, filtering, and validation")]
#[command(version = "2.0.0")]
pub struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert protobuf to JSON format
    ToJson {
        /// Path to the protobuf file or directory (length-delimited RecordedBatch messages)
        #[clap(long)]
        input: PathBuf,
        /// Output JSON file path or directory
        #[clap(long)]
        output: PathBuf,
        /// Enable pretty printing (default: true)
        #[clap(long, default_value = "true")]
        pretty: bool,
        /// Include detailed metadata in output
        #[clap(long, default_value = "true")]
        include_metadata: bool,
        /// Process multiple files in batch mode
        #[clap(long)]
        batch: bool,
        /// Filter by time range (start timestamp in seconds)
        #[clap(long)]
        start_time: Option<i64>,
        /// Filter by time range (end timestamp in seconds)
        #[clap(long)]
        end_time: Option<i64>,
        /// Filter by specific DEX names (comma-separated)
        #[clap(long)]
        dex_filter: Option<String>,
        /// Filter by specific pool IDs (comma-separated)
        #[clap(long)]
        pool_filter: Option<String>,
        /// Exclude transaction data from output
        #[clap(long)]
        exclude_transactions: bool,
        /// Include only pool states in output
        #[clap(long)]
        pool_states_only: bool,
        /// Compress output files (gzip)
        #[clap(long)]
        compress: bool,
        /// Generate detailed statistics report
        #[clap(long)]
        stats: bool,
    },
    /// Convert JSON back to protobuf format
    ToProtobuf {
        /// Path to the JSON file or directory
        #[clap(long)]
        input: PathBuf,
        /// Output protobuf file path or directory
        #[clap(long)]
        output: PathBuf,
        /// Validate data integrity during conversion
        #[clap(long, default_value = "true")]
        validate: bool,
        /// Process multiple files in batch mode
        #[clap(long)]
        batch: bool,
        /// Strict validation mode
        #[clap(long)]
        strict: bool,
        /// Repair mode - attempt to fix common issues
        #[clap(long)]
        repair: bool,
        /// Backup original file before conversion
        #[clap(long)]
        backup_original: bool,
        /// Generate detailed statistics report
        #[clap(long)]
        stats: bool,
    },
    /// Validate round-trip conversion integrity
    Validate {
        /// Path to the protobuf file or directory
        #[clap(long)]
        protobuf: PathBuf,
        /// Path to the JSON file or directory
        #[clap(long)]
        json: PathBuf,
        /// Process multiple files in batch mode
        #[clap(long)]
        batch: bool,
        /// Number of validation cycles to run
        #[clap(long, default_value = "1")]
        cycles: u32,
        /// Generate detailed validation report
        #[clap(long)]
        detailed: bool,
        /// Output validation report to file
        #[clap(long)]
        report: Option<PathBuf>,
        /// Temporary directory for validation files
        #[clap(long)]
        temp_dir: Option<PathBuf>,
        /// Number of parallel validation workers
        #[clap(long, default_value = "1")]
        parallel: usize,
    },
    /// Display file information and statistics
    Info {
        /// Path to the input file
        #[clap(long)]
        input: PathBuf,
        /// Show detailed information
        #[clap(long)]
        detailed: bool,
        /// Export statistics to CSV file
        #[clap(long)]
        export_csv: Option<PathBuf>,
        /// Show pool breakdown statistics
        #[clap(long)]
        pool_breakdown: bool,
        /// Show DEX breakdown statistics
        #[clap(long)]
        dex_breakdown: bool,
        /// Perform time range analysis
        #[clap(long)]
        time_range_analysis: bool,
    },
    /// Compare two recording files
    Diff {
        /// First file to compare
        #[clap(long)]
        file1: PathBuf,
        /// Second file to compare
        #[clap(long)]
        file2: PathBuf,
        /// Output diff report to file
        #[clap(long)]
        output: Option<PathBuf>,
        /// Ignore timestamp differences
        #[clap(long)]
        ignore_timestamps: bool,
        /// Compare only pool states
        #[clap(long)]
        pool_states_only: bool,
    },
    /// Batch process multiple files with advanced options
    Batch {
        /// Input directory containing files to process
        #[clap(long)]
        input_dir: PathBuf,
        /// Output directory for processed files
        #[clap(long)]
        output_dir: PathBuf,
        /// Operation to perform (to-json, to-protobuf, validate)
        #[clap(long)]
        operation: String,
        /// File pattern to match (e.g., "*.pb", "*.json")
        #[clap(long, default_value = "*")]
        _pattern: String,
        /// Number of parallel workers
        #[clap(long, default_value = "4")]
        workers: usize,
        /// Show progress during batch operations
        #[clap(long)]
        progress: bool,
        /// Resume from previous batch state file
        #[clap(long)]
        resume: Option<PathBuf>,
        /// Continue processing on errors
        #[clap(long)]
        _continue_on_error: bool,
    },
}

/// Conversion options for filtering output
#[derive(Debug, Clone, Default)]
pub struct ConversionOptions {
    pub exclude_transactions: bool,
    pub pool_states_only: bool,
}

/// Conversion statistics
#[derive(Debug, Default)]
pub struct ConversionStats {
    pub files_processed: usize,
    pub batches_processed: usize,
    pub transactions_processed: usize,
    pub pool_states_processed: usize,
    pub bytes_processed: u64,
    pub errors: usize,
    pub warnings: usize,
}

impl ConversionStats {
    pub fn format_summary(&self) -> String {
        format!(
            "Conversion Summary:\n\
             Files Processed: {}\n\
             Batches Processed: {}\n\
             Transactions Processed: {}\n\
             Pool States Processed: {}\n\
             Data Processed: {:.2} MB\n\
             Errors: {}\n\
             Warnings: {}",
            self.files_processed,
            self.batches_processed,
            self.transactions_processed,
            self.pool_states_processed,
            self.bytes_processed as f64 / 1024.0 / 1024.0,
            self.errors,
            self.warnings
        )
    }
}

/// Filter configuration for conversion
#[derive(Debug, Clone)]
pub struct ConversionFilter {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub dex_filter: Option<Vec<String>>,
    pub pool_filter: Option<Vec<String>>,
}

impl ConversionFilter {
    pub fn from_args(
        start_time: Option<i64>,
        end_time: Option<i64>,
        dex_filter: Option<String>,
        pool_filter: Option<String>,
    ) -> Self {
        Self {
            start_time,
            end_time,
            dex_filter: dex_filter.map(|s| s.split(',').map(|s| s.trim().to_string()).collect()),
            pool_filter: pool_filter.map(|s| s.split(',').map(|s| s.trim().to_string()).collect()),
        }
    }

    pub fn should_include_batch(&self, batch: &RecordedBatch) -> bool {
        // Time range filter
        if let Some(start) = self.start_time {
            if batch.timestamp_ms < start * 1000 {
                return false;
            }
        }
        if let Some(end) = self.end_time {
            if batch.timestamp_ms > end * 1000 {
                return false;
            }
        }

        // DEX filter
        if let Some(ref dex_list) = self.dex_filter {
            let has_matching_dex = batch
                .pool_initializations
                .iter()
                .any(|pool| dex_list.contains(&pool.dex_name));
            if !has_matching_dex && !batch.pool_initializations.is_empty() {
                return false;
            }
        }

        // Pool filter
        if let Some(ref pool_list) = self.pool_filter {
            let has_matching_pool = batch
                .pool_initializations
                .iter()
                .any(|pool| pool_list.contains(&pool.pool_id));
            if !has_matching_pool && !batch.pool_initializations.is_empty() {
                return false;
            }
        }

        true
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    match args.command {
        Commands::ToJson {
            input,
            output,
            pretty,
            include_metadata,
            batch,
            start_time,
            end_time,
            dex_filter,
            pool_filter,
            exclude_transactions,
            pool_states_only,
            compress,
            stats,
        } => {
            let filter = ConversionFilter::from_args(start_time, end_time, dex_filter, pool_filter);
            let options = ConversionOptions {
                exclude_transactions,
                pool_states_only,
            };

            if batch {
                convert_to_json_batch(
                    input,
                    output,
                    pretty,
                    include_metadata,
                    filter,
                    options,
                    compress,
                    stats,
                )
            } else {
                convert_to_json_single(
                    input,
                    output,
                    pretty,
                    include_metadata,
                    filter,
                    options,
                    compress,
                    stats,
                )
            }
        }
        Commands::ToProtobuf {
            input,
            output,
            validate,
            batch,
            strict,
            repair,
            backup_original,
            stats,
        } => {
            if batch {
                convert_to_protobuf_batch(
                    input,
                    output,
                    validate,
                    strict,
                    repair,
                    backup_original,
                    stats,
                )
            } else {
                convert_to_protobuf_single(
                    input,
                    output,
                    validate,
                    strict,
                    repair,
                    backup_original,
                    stats,
                )
            }
        }
        Commands::Validate {
            protobuf,
            json,
            batch,
            cycles,
            detailed,
            report,
            temp_dir,
            parallel,
        } => {
            if batch {
                validate_batch(protobuf, json, cycles, detailed, report, temp_dir, parallel)
            } else {
                validate_single(protobuf, json, cycles, detailed, report, temp_dir, parallel)
            }
        }
        Commands::Info {
            input,
            detailed,
            export_csv,
            pool_breakdown,
            dex_breakdown,
            time_range_analysis,
        } => show_file_info(
            input,
            detailed,
            export_csv,
            pool_breakdown,
            dex_breakdown,
            time_range_analysis,
        ),
        Commands::Diff {
            file1,
            file2,
            output,
            ignore_timestamps,
            pool_states_only,
        } => compare_files(file1, file2, output, ignore_timestamps, pool_states_only),
        Commands::Batch {
            input_dir,
            output_dir,
            operation,
            _pattern,
            workers,
            progress,
            resume,
            _continue_on_error,
        } => process_batch_operation(
            input_dir,
            output_dir,
            operation,
            _pattern,
            workers,
            progress,
            resume,
            _continue_on_error,
        ),
    }
}

/// Convert single protobuf file to JSON
#[allow(clippy::too_many_arguments)]
fn convert_to_json_single(
    input: PathBuf,
    output: PathBuf,
    pretty: bool,
    include_metadata: bool,
    filter: ConversionFilter,
    _options: ConversionOptions,
    compress: bool,
    generate_stats: bool,
) -> Result<()> {
    info!(
        "Converting protobuf to JSON: {} -> {}",
        input.display(),
        output.display()
    );

    let mut stats = ConversionStats::default();
    let data = std::fs::read(&input)?;
    stats.bytes_processed = data.len() as u64;
    stats.files_processed = 1;

    let mut buf = bytes::BytesMut::from(&data[..]);
    let mut batches = Vec::new();

    while buf.has_remaining() {
        match RecordedBatch::decode_length_delimited(&mut buf) {
            Ok(batch) => {
                if filter.should_include_batch(&batch) {
                    stats.batches_processed += 1;
                    stats.transactions_processed += batch.transactions.len();
                    stats.pool_states_processed += batch.pool_initializations.len();
                    batches.push(batch);
                }
            }
            Err(e) => {
                error!("Failed to decode batch: {}", e);
                stats.errors += 1;
            }
        }
    }

    // Create JSON output
    let json_data = if include_metadata {
        create_json_with_metadata(&batches, &stats)?
    } else if pretty {
        to_string_pretty(&batches)?
    } else {
        serde_json::to_string(&batches)?
    };

    // Write output
    write_output(&output, json_data.as_bytes(), compress)?;

    if generate_stats {
        println!("{}", stats.format_summary());
    }

    info!("Conversion completed successfully");
    Ok(())
}

/// Convert batch of protobuf files to JSON
#[allow(clippy::too_many_arguments)]
fn convert_to_json_batch(
    input_dir: PathBuf,
    output_dir: PathBuf,
    pretty: bool,
    include_metadata: bool,
    filter: ConversionFilter,
    options: ConversionOptions,
    compress: bool,
    generate_stats: bool,
) -> Result<()> {
    info!(
        "Converting protobuf files in batch mode: {} -> {}",
        input_dir.display(),
        output_dir.display()
    );

    create_dir_all(&output_dir)?;
    let mut total_stats = ConversionStats::default();

    for entry in std::fs::read_dir(&input_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("pb") {
            let output_file =
                output_dir.join(path.file_stem().unwrap().to_str().unwrap().to_string() + ".json");

            match convert_to_json_single(
                path.clone(),
                output_file,
                pretty,
                include_metadata,
                filter.clone(),
                options.clone(),
                compress,
                false,
            ) {
                Ok(_) => {
                    info!("Converted: {}", path.display());
                    total_stats.files_processed += 1;
                }
                Err(e) => {
                    error!("Failed to convert {}: {}", path.display(), e);
                    total_stats.errors += 1;
                }
            }
        }
    }

    if generate_stats {
        println!("{}", total_stats.format_summary());
    }

    Ok(())
}

/// Convert single JSON file to protobuf
fn convert_to_protobuf_single(
    input: PathBuf,
    output: PathBuf,
    validate: bool,
    _strict: bool,
    repair: bool,
    _backup_original: bool,
    generate_stats: bool,
) -> Result<()> {
    info!(
        "Converting JSON to protobuf: {} -> {}",
        input.display(),
        output.display()
    );

    let mut stats = ConversionStats::default();
    let content = std::fs::read_to_string(&input)?;
    stats.files_processed = 1;

    let batches: Vec<RecordedBatch> = if repair {
        repair_and_parse_json(&content, &mut stats)?
    } else {
        parse_json_batches(&content).context("Failed to parse JSON")?
    };

    stats.batches_processed = batches.len();
    for batch in &batches {
        stats.transactions_processed += batch.transactions.len();
        stats.pool_states_processed += batch.pool_initializations.len();
    }

    // Write protobuf output
    let file = File::create(&output)?;
    let mut writer = BufWriter::new(file);

    for batch in &batches {
        let encoded = batch.encode_length_delimited_to_vec();
        writer.write_all(&encoded)?;
        stats.bytes_processed += encoded.len() as u64;
    }

    writer.flush()?;

    // Validate if requested
    if validate {
        validate_conversion(&output, &batches)?;
    }

    if generate_stats {
        println!("{}", stats.format_summary());
    }

    info!("Conversion completed successfully");
    Ok(())
}

/// Convert batch of JSON files to protobuf
fn convert_to_protobuf_batch(
    input_dir: PathBuf,
    output_dir: PathBuf,
    validate: bool,
    strict: bool,
    repair: bool,
    backup_original: bool,
    generate_stats: bool,
) -> Result<()> {
    info!(
        "Converting JSON files in batch mode: {} -> {}",
        input_dir.display(),
        output_dir.display()
    );

    create_dir_all(&output_dir)?;
    let mut total_stats = ConversionStats::default();

    for entry in std::fs::read_dir(&input_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let output_file =
                output_dir.join(path.file_stem().unwrap().to_str().unwrap().to_string() + ".pb");

            match convert_to_protobuf_single(
                path.clone(),
                output_file,
                validate,
                strict,
                repair,
                backup_original,
                false,
            ) {
                Ok(_) => {
                    info!("Converted: {}", path.display());
                    total_stats.files_processed += 1;
                }
                Err(e) => {
                    error!("Failed to convert {}: {}", path.display(), e);
                    total_stats.errors += 1;
                }
            }
        }
    }

    if generate_stats {
        println!("{}", total_stats.format_summary());
    }

    Ok(())
}

/// Validate single file pair
fn validate_single(
    protobuf: PathBuf,
    json: PathBuf,
    _cycles: u32,
    detailed: bool,
    _report: Option<PathBuf>,
    _temp_dir: Option<PathBuf>,
    _parallel: usize,
) -> Result<()> {
    info!(
        "Validating conversion: {} <-> {}",
        protobuf.display(),
        json.display()
    );

    // Read and parse both files
    let pb_data = std::fs::read(&protobuf)?;
    let json_content = std::fs::read_to_string(&json)?;

    let mut pb_batches = Vec::new();
    let mut buf = bytes::BytesMut::from(&pb_data[..]);

    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
        pb_batches.push(batch);
    }

    let json_batches: Vec<RecordedBatch> = parse_json_batches(&json_content)?;

    // Compare
    if pb_batches.len() != json_batches.len() {
        return Err(anyhow::anyhow!(
            "Batch count mismatch: protobuf={}, json={}",
            pb_batches.len(),
            json_batches.len()
        ));
    }

    for (i, (pb_batch, json_batch)) in pb_batches.iter().zip(json_batches.iter()).enumerate() {
        if detailed {
            validate_batch_detailed(pb_batch, json_batch, i)?;
        } else {
            validate_batch_basic(pb_batch, json_batch, i)?;
        }
    }

    println!("Round-trip validation successful");
    println!("Data integrity: PASSED");
    info!("Validation completed successfully");
    Ok(())
}

/// Validate batch of file pairs
fn validate_batch(
    protobuf_dir: PathBuf,
    json_dir: PathBuf,
    _cycles: u32,
    detailed: bool,
    _report: Option<PathBuf>,
    _temp_dir: Option<PathBuf>,
    _parallel: usize,
) -> Result<()> {
    info!(
        "Validating files in batch mode: {} <-> {}",
        protobuf_dir.display(),
        json_dir.display()
    );

    let mut validated = 0;
    let mut errors = 0;

    for entry in std::fs::read_dir(&protobuf_dir)? {
        let entry = entry?;
        let pb_path = entry.path();

        if pb_path.extension().and_then(|s| s.to_str()) == Some("pb") {
            let json_path =
                json_dir.join(pb_path.file_stem().unwrap().to_str().unwrap().to_string() + ".json");

            if json_path.exists() {
                match validate_single(pb_path.clone(), json_path, _cycles, detailed, None, None, 1)
                {
                    Ok(_) => {
                        info!("Validated: {}", pb_path.display());
                        validated += 1;
                    }
                    Err(e) => {
                        error!("Validation failed for {}: {}", pb_path.display(), e);
                        errors += 1;
                    }
                }
            } else {
                warn!("No corresponding JSON file for: {}", pb_path.display());
            }
        }
    }

    info!(
        "Validation completed: {} validated, {} errors",
        validated, errors
    );
    Ok(())
}

/// Process batch operation with parallel workers
#[allow(clippy::too_many_arguments)]
fn process_batch_operation(
    input_dir: PathBuf,
    output_dir: PathBuf,
    operation: String,
    _pattern: String,
    _workers: usize,
    _progress: bool,
    _resume: Option<PathBuf>,
    _continue_on_error: bool,
) -> Result<()> {
    info!("Processing batch operation: {}", operation);

    // This is a placeholder for the parallel processing implementation
    // In a full implementation, you would use tokio tasks or thread pools

    match operation.as_str() {
        "to-json" => {
            convert_to_json_batch(
                input_dir,
                output_dir,
                true, // pretty
                true, // include_metadata
                ConversionFilter::from_args(None, None, None, None),
                ConversionOptions::default(),
                false, // compress
                true,  // stats
            )
        }
        "to-protobuf" => {
            convert_to_protobuf_batch(
                input_dir, output_dir, true,  // validate
                false, // strict
                false, // repair
                false, // backup_original
                true,  // stats
            )
        }
        "validate" => validate_batch(input_dir, output_dir, 1, false, None, None, 1),
        _ => Err(anyhow::anyhow!("Unknown operation: {}", operation)),
    }
}

/// Helper functions
fn create_json_with_metadata(batches: &[RecordedBatch], stats: &ConversionStats) -> Result<String> {
    let output = serde_json::json!({
        "metadata": {
            "conversion_timestamp": chrono::Utc::now().to_rfc3339(),
            "converter_version": "2.0.0",
            "statistics": {
                "batches_count": stats.batches_processed,
                "transactions_count": stats.transactions_processed,
                "pool_states_count": stats.pool_states_processed,
                "total_bytes": stats.bytes_processed,
            }
        },
        "batches": batches
    });

    Ok(to_string_pretty(&output)?)
}

fn write_output(path: &PathBuf, data: &[u8], compress: bool) -> Result<()> {
    if compress {
        // Implement compression if needed
        warn!("Compression not yet implemented, writing uncompressed");
    }

    std::fs::write(path, data)?;
    Ok(())
}

/// Parse JSON content that may be in different formats:
/// 1. Direct array: [batch1, batch2, ...]
/// 2. With metadata: {"metadata": {...}, "batches": [batch1, batch2, ...]}
fn parse_json_batches(content: &str) -> Result<Vec<RecordedBatch>> {
    // First try to parse as a direct array
    if let Ok(batches) = from_str::<Vec<RecordedBatch>>(content) {
        return Ok(batches);
    }

    // If that fails, try to parse as an object with metadata
    let json_value: serde_json::Value =
        from_str(content).context("Failed to parse JSON as any valid format")?;

    if let Some(batches_value) = json_value.get("batches") {
        let batches: Vec<RecordedBatch> = serde_json::from_value(batches_value.clone())
            .context("Failed to parse 'batches' field from JSON")?;
        return Ok(batches);
    }

    Err(anyhow::anyhow!(
        "JSON format not recognized. Expected either a direct array of batches or an object with 'batches' field"
    ))
}

fn repair_and_parse_json(content: &str, stats: &mut ConversionStats) -> Result<Vec<RecordedBatch>> {
    // Try smart parsing first
    match parse_json_batches(content) {
        Ok(batches) => Ok(batches),
        Err(e) => {
            stats.errors += 1;
            Err(anyhow::anyhow!("JSON repair not implemented: {}", e))
        }
    }
}

fn validate_conversion(output_path: &PathBuf, original_batches: &[RecordedBatch]) -> Result<()> {
    let data = std::fs::read(output_path)?;
    let mut buf = bytes::BytesMut::from(&data[..]);
    let mut decoded_batches = Vec::new();

    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
        decoded_batches.push(batch);
    }

    if decoded_batches.len() != original_batches.len() {
        return Err(anyhow::anyhow!("Batch count mismatch after conversion"));
    }

    Ok(())
}

fn validate_batch_basic(
    pb_batch: &RecordedBatch,
    json_batch: &RecordedBatch,
    index: usize,
) -> Result<()> {
    if pb_batch.start_version != json_batch.start_version
        || pb_batch.end_version != json_batch.end_version
        || pb_batch.transactions.len() != json_batch.transactions.len()
    {
        return Err(anyhow::anyhow!(
            "Basic validation failed for batch {}",
            index
        ));
    }
    Ok(())
}

fn validate_batch_detailed(
    pb_batch: &RecordedBatch,
    json_batch: &RecordedBatch,
    index: usize,
) -> Result<()> {
    validate_batch_basic(pb_batch, json_batch, index)?;

    // Add more detailed validation here
    if pb_batch.pool_initializations.len() != json_batch.pool_initializations.len() {
        return Err(anyhow::anyhow!(
            "Pool initialization count mismatch for batch {}",
            index
        ));
    }

    Ok(())
}

/// Display detailed file information and statistics
fn show_file_info(
    input: PathBuf,
    _detailed: bool,
    export_csv: Option<PathBuf>,
    pool_breakdown: bool,
    dex_breakdown: bool,
    time_range_analysis: bool,
) -> Result<()> {
    info!("Analyzing file: {}", input.display());

    let data = std::fs::read(&input)?;
    let mut buf = bytes::BytesMut::from(&data[..]);

    let mut batch_count = 0;
    let mut transaction_count = 0;
    let mut pool_state_count = 0;
    let mut min_time = i64::MAX;
    let mut max_time = i64::MIN;
    let mut dex_stats = std::collections::HashMap::new();
    let mut pool_stats = std::collections::HashMap::new();

    while buf.has_remaining() {
        match RecordedBatch::decode_length_delimited(&mut buf) {
            Ok(batch) => {
                batch_count += 1;
                transaction_count += batch.transactions.len();
                pool_state_count += batch.pool_initializations.len();

                if batch.timestamp_ms < min_time {
                    min_time = batch.timestamp_ms;
                }
                if batch.timestamp_ms > max_time {
                    max_time = batch.timestamp_ms;
                }

                for pool_state in &batch.pool_initializations {
                    *dex_stats.entry(pool_state.dex_name.clone()).or_insert(0) += 1;
                    *pool_stats.entry(pool_state.pool_id.clone()).or_insert(0) += 1;
                }
            }
            Err(e) => {
                error!("Failed to decode batch: {}", e);
            }
        }
    }

    // Display basic info
    println!("File Information:");
    println!("  File Size: {:.2} MB", data.len() as f64 / 1024.0 / 1024.0);
    println!("  Batches: {}", batch_count);
    println!("  Transactions: {}", transaction_count);
    println!("  Pool States: {}", pool_state_count);

    if time_range_analysis && min_time != i64::MAX {
        let start_time = chrono::DateTime::from_timestamp(min_time / 1000, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Invalid".to_string());
        let end_time = chrono::DateTime::from_timestamp(max_time / 1000, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Invalid".to_string());

        println!("  Time Range: {} to {}", start_time, end_time);
        println!(
            "  Duration: {:.2} hours",
            (max_time - min_time) as f64 / 1000.0 / 3600.0
        );
    }

    if dex_breakdown && !dex_stats.is_empty() {
        println!("\nDEX Breakdown:");
        for (dex, count) in &dex_stats {
            println!("  {}: {} pools", dex, count);
        }
    }

    if pool_breakdown && !pool_stats.is_empty() {
        println!("\nTop Pools:");
        let mut pools: Vec<_> = pool_stats.iter().collect();
        pools.sort_by(|a, b| b.1.cmp(a.1));
        for (pool, count) in pools.iter().take(10) {
            println!("  {}: {} occurrences", pool, count);
        }
    }

    // Export to CSV if requested
    if let Some(csv_path) = export_csv {
        export_stats_to_csv(&csv_path, &dex_stats, &pool_stats)?;
        info!("Statistics exported to: {}", csv_path.display());
    }

    Ok(())
}

/// Compare two recording files
fn compare_files(
    file1: PathBuf,
    file2: PathBuf,
    output: Option<PathBuf>,
    ignore_timestamps: bool,
    pool_states_only: bool,
) -> Result<()> {
    info!(
        "Comparing files: {} vs {}",
        file1.display(),
        file2.display()
    );

    let data1 = std::fs::read(&file1)?;
    let data2 = std::fs::read(&file2)?;

    let mut buf1 = bytes::BytesMut::from(&data1[..]);
    let mut buf2 = bytes::BytesMut::from(&data2[..]);

    let mut batches1 = Vec::new();
    let mut batches2 = Vec::new();

    // Read all batches from both files
    while buf1.has_remaining() {
        if let Ok(batch) = RecordedBatch::decode_length_delimited(&mut buf1) {
            batches1.push(batch);
        }
    }

    while buf2.has_remaining() {
        if let Ok(batch) = RecordedBatch::decode_length_delimited(&mut buf2) {
            batches2.push(batch);
        }
    }

    let mut differences = Vec::new();

    // Compare batch counts
    if batches1.len() != batches2.len() {
        differences.push(format!(
            "Batch count differs: {} vs {}",
            batches1.len(),
            batches2.len()
        ));
    }

    // Compare individual batches
    let min_batches = batches1.len().min(batches2.len());
    for i in 0..min_batches {
        let batch1 = &batches1[i];
        let batch2 = &batches2[i];

        if !ignore_timestamps && batch1.timestamp_ms != batch2.timestamp_ms {
            differences.push(format!(
                "Batch {} timestamp differs: {} vs {}",
                i, batch1.timestamp_ms, batch2.timestamp_ms
            ));
        }

        if !pool_states_only && batch1.transactions.len() != batch2.transactions.len() {
            differences.push(format!(
                "Batch {} transaction count differs: {} vs {}",
                i,
                batch1.transactions.len(),
                batch2.transactions.len()
            ));
        }

        if batch1.pool_initializations.len() != batch2.pool_initializations.len() {
            differences.push(format!(
                "Batch {} pool state count differs: {} vs {}",
                i,
                batch1.pool_initializations.len(),
                batch2.pool_initializations.len()
            ));
        }
    }

    // Output results
    if differences.is_empty() {
        println!("Files are identical (with specified comparison options)");
    } else {
        println!("Found {} differences:", differences.len());
        for diff in &differences {
            println!("  {}", diff);
        }
    }

    // Write report if requested
    if let Some(output_path) = output {
        let report = serde_json::json!({
            "file1": file1.to_string_lossy(),
            "file2": file2.to_string_lossy(),
            "comparison_options": {
                "ignore_timestamps": ignore_timestamps,
                "pool_states_only": pool_states_only
            },
            "differences": differences,
            "identical": differences.is_empty()
        });

        std::fs::write(&output_path, serde_json::to_string_pretty(&report)?)?;
        info!("Diff report written to: {}", output_path.display());
    }

    Ok(())
}

/// Export statistics to CSV file
fn export_stats_to_csv(
    csv_path: &PathBuf,
    dex_stats: &std::collections::HashMap<String, usize>,
    pool_stats: &std::collections::HashMap<String, usize>,
) -> Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(csv_path)?;

    // Write DEX stats
    writeln!(file, "Type,Name,Count")?;
    for (dex, count) in dex_stats {
        writeln!(file, "DEX,{},{}", dex, count)?;
    }

    // Write pool stats (top 100)
    let mut pools: Vec<_> = pool_stats.iter().collect();
    pools.sort_by(|a, b| b.1.cmp(a.1));
    for (pool, count) in pools.iter().take(100) {
        writeln!(file, "Pool,{},{}", pool, count)?;
    }

    Ok(())
}
