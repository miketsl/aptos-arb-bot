use anyhow::{Context, Result};
use bytes::Buf;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_json::{to_string_pretty, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use market_data_ingestor::data_source::RecordedBatch;

/// mdi-inspector: comprehensive data analysis and inspection tool
#[derive(Parser)]
#[command(name = "mdi-inspector")]
#[command(about = "Data analysis and inspection tool for recorded market data")]
#[command(version = "1.0.0")]
pub struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform comprehensive analysis of recording data
    Analyze {
        /// Path to the input recording file
        #[clap(long)]
        input: PathBuf,
        /// Comma-separated list of analyzers to run (default: all)
        #[clap(long)]
        analyzers: Option<String>,
        /// Export results as JSON
        #[clap(long)]
        export_json: bool,
        /// Export results as CSV
        #[clap(long)]
        export_csv: bool,
        /// Output directory for analysis results
        #[clap(long, default_value = "analysis")]
        output_dir: PathBuf,
        /// Generate summary report
        #[clap(long)]
        report: Option<PathBuf>,
    },
    /// Pool-specific analysis with filtering
    Pools {
        /// Path to the input recording file
        #[clap(long)]
        input: PathBuf,
        /// Filter by DEX name
        #[clap(long)]
        dex: Option<String>,
        /// Filter by pool type (clmm, weighted, stable)
        #[clap(long)]
        pool_type: Option<String>,
        /// Time range filter (start,end in ISO 8601 format)
        #[clap(long)]
        time_range: Option<String>,
        /// Export results to CSV file
        #[clap(long)]
        export: Option<PathBuf>,
    },
    /// Timeline analysis with configurable granularity
    Timeline {
        /// Path to the input recording file
        #[clap(long)]
        input: PathBuf,
        /// Granularity for timeline analysis (1s, 1m, 1h)
        #[clap(long, default_value = "1m")]
        granularity: String,
        /// Metrics to analyze (transactions,pools,api_calls)
        #[clap(long, default_value = "transactions,pools")]
        metrics: String,
        /// Output chart file (PNG format) - placeholder for future implementation
        #[clap(long)]
        chart: Option<PathBuf>,
        /// Export timeline data to CSV
        #[clap(long)]
        export: Option<PathBuf>,
    },
    /// Data quality checks and validation
    Quality {
        /// Path to the input recording file
        #[clap(long)]
        input: PathBuf,
        /// Specific checks to run (comma-separated)
        #[clap(
            long,
            default_value = "missing_pools,version_gaps,timestamp_consistency"
        )]
        checks: String,
        /// Output validation report
        #[clap(long)]
        report: Option<PathBuf>,
        /// Detailed quality analysis
        #[clap(long)]
        detailed: bool,
    },
}

/// Core data inspector with pluggable analyzers and exporters
pub struct DataInspector {
    analyzers: HashMap<String, Box<dyn DataAnalyzer>>,
    exporters: HashMap<String, Box<dyn DataExporter>>,
}

impl Default for DataInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl DataInspector {
    pub fn new() -> Self {
        let mut inspector = Self {
            analyzers: HashMap::new(),
            exporters: HashMap::new(),
        };

        // Register built-in analyzers
        inspector.register_analyzer("transaction", Box::new(TransactionAnalyzer::new()));
        inspector.register_analyzer("pool", Box::new(PoolAnalyzer::new()));
        inspector.register_analyzer("dex", Box::new(DEXAnalyzer::new()));
        inspector.register_analyzer("timerange", Box::new(TimeRangeAnalyzer::new()));
        inspector.register_analyzer("quality", Box::new(DataQualityAnalyzer::new()));

        // Register built-in exporters
        inspector.register_exporter("json", Box::new(JsonExporter::new()));
        inspector.register_exporter("csv", Box::new(CsvExporter::new()));

        inspector
    }

    pub fn register_analyzer(&mut self, name: &str, analyzer: Box<dyn DataAnalyzer>) {
        self.analyzers.insert(name.to_string(), analyzer);
    }

    pub fn register_exporter(&mut self, format: &str, exporter: Box<dyn DataExporter>) {
        self.exporters.insert(format.to_string(), exporter);
    }

    pub fn run_analysis(
        &self,
        data: &RecordingData,
        analyzer_names: &[String],
    ) -> Result<Vec<AnalysisResult>> {
        let mut results = Vec::new();

        for name in analyzer_names {
            if let Some(analyzer) = self.analyzers.get(name) {
                info!("Running {} analyzer...", name);
                match analyzer.analyze(data) {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        error!("Analyzer {} failed: {}", name, e);
                        return Err(e);
                    }
                }
            } else {
                warn!("Unknown analyzer: {}", name);
            }
        }

        Ok(results)
    }

    pub fn export_results(
        &self,
        results: &[AnalysisResult],
        format: &str,
        output: &Path,
    ) -> Result<()> {
        if let Some(exporter) = self.exporters.get(format) {
            exporter.export(results, output)
        } else {
            Err(anyhow::anyhow!("Unknown export format: {}", format))
        }
    }

    pub fn get_available_analyzers(&self) -> Vec<String> {
        self.analyzers.keys().cloned().collect()
    }
}

/// Trait for data analyzers
pub trait DataAnalyzer: Send + Sync {
    fn name(&self) -> &'static str;
    fn analyze(&self, data: &RecordingData) -> Result<AnalysisResult>;
}

/// Trait for data exporters
pub trait DataExporter: Send + Sync {
    fn format(&self) -> &'static str;
    fn export(&self, results: &[AnalysisResult], output: &Path) -> Result<()>;
}

/// Container for recording data loaded from file
#[derive(Debug)]
pub struct RecordingData {
    pub batches: Vec<RecordedBatch>,
    pub file_size: u64,
    pub load_time: std::time::Duration,
}

impl RecordingData {
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        let start = std::time::Instant::now();
        let data = std::fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        let file_size = data.len() as u64;

        let mut buf = bytes::BytesMut::from(&data[..]);
        let mut batches = Vec::new();

        while buf.has_remaining() {
            match RecordedBatch::decode_length_delimited(&mut buf) {
                Ok(batch) => batches.push(batch),
                Err(e) => {
                    warn!("Failed to decode batch: {}", e);
                    break;
                }
            }
        }

        let load_time = start.elapsed();
        info!(
            "Loaded {} batches from {} ({:.2} MB) in {:?}",
            batches.len(),
            path.display(),
            file_size as f64 / 1024.0 / 1024.0,
            load_time
        );

        Ok(Self {
            batches,
            file_size,
            load_time,
        })
    }

    pub fn total_transactions(&self) -> usize {
        self.batches.iter().map(|b| b.transactions.len()).sum()
    }

    pub fn total_pool_states(&self) -> usize {
        self.batches
            .iter()
            .map(|b| b.pool_initializations.len())
            .sum()
    }

    pub fn time_range(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        if self.batches.is_empty() {
            return None;
        }

        let start_time = self.batches.iter().map(|b| b.timestamp_ms).min()?;
        let end_time = self.batches.iter().map(|b| b.timestamp_ms).max()?;

        let start_dt = DateTime::from_timestamp(start_time / 1000, 0)?;
        let end_dt = DateTime::from_timestamp(end_time / 1000, 0)?;

        Some((start_dt, end_dt))
    }

    pub fn duration_seconds(&self) -> Option<f64> {
        self.time_range()
            .map(|(start, end)| (end - start).num_seconds() as f64)
    }
}

/// Result from running an analyzer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub analyzer_name: String,
    pub summary: String,
    pub metrics: HashMap<String, Value>,
    pub details: Vec<AnalysisDetail>,
}

/// Individual analysis detail item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisDetail {
    pub category: String,
    pub name: String,
    pub value: Value,
    pub description: Option<String>,
}

// Built-in Analyzers

/// Analyzes transaction patterns and processing metrics
pub struct TransactionAnalyzer;

impl Default for TransactionAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl DataAnalyzer for TransactionAnalyzer {
    fn name(&self) -> &'static str {
        "transaction"
    }

    fn analyze(&self, data: &RecordingData) -> Result<AnalysisResult> {
        let mut metrics = HashMap::new();
        let mut details = Vec::new();

        let total_transactions = data.total_transactions();
        let batch_count = data.batches.len();

        metrics.insert(
            "total_transactions".to_string(),
            serde_json::json!(total_transactions),
        );
        metrics.insert("total_batches".to_string(), serde_json::json!(batch_count));

        // Calculate transaction rate
        if let Some(duration_secs) = data.duration_seconds() {
            if duration_secs > 0.0 {
                let tx_rate = total_transactions as f64 / duration_secs;
                metrics.insert(
                    "transactions_per_second".to_string(),
                    serde_json::json!(tx_rate),
                );
                details.push(AnalysisDetail {
                    category: "throughput".to_string(),
                    name: "transaction_rate".to_string(),
                    value: serde_json::json!(format!("{:.2} tx/sec", tx_rate)),
                    description: Some("Average transaction processing rate".to_string()),
                });
            }
        }

        // Analyze batch sizes
        let batch_sizes: Vec<usize> = data.batches.iter().map(|b| b.transactions.len()).collect();
        if !batch_sizes.is_empty() {
            let avg_batch_size =
                batch_sizes.iter().sum::<usize>() as f64 / batch_sizes.len() as f64;
            let min_batch_size = *batch_sizes.iter().min().unwrap();
            let max_batch_size = *batch_sizes.iter().max().unwrap();

            metrics.insert(
                "average_batch_size".to_string(),
                serde_json::json!(avg_batch_size),
            );
            metrics.insert(
                "min_batch_size".to_string(),
                serde_json::json!(min_batch_size),
            );
            metrics.insert(
                "max_batch_size".to_string(),
                serde_json::json!(max_batch_size),
            );

            details.push(AnalysisDetail {
                category: "batch_analysis".to_string(),
                name: "batch_size_stats".to_string(),
                value: serde_json::json!({
                    "average": avg_batch_size,
                    "min": min_batch_size,
                    "max": max_batch_size
                }),
                description: Some("Batch size statistics".to_string()),
            });
        }

        // Check for version gaps
        let mut version_gaps = Vec::new();
        for window in data.batches.windows(2) {
            let current_end = window[0].end_version;
            let next_start = window[1].start_version;
            if next_start != current_end + 1 {
                version_gaps.push((current_end, next_start));
            }
        }

        metrics.insert(
            "version_gaps_count".to_string(),
            serde_json::json!(version_gaps.len()),
        );
        if !version_gaps.is_empty() {
            details.push(AnalysisDetail {
                category: "integrity".to_string(),
                name: "version_gaps".to_string(),
                value: serde_json::json!(version_gaps),
                description: Some(
                    "Gaps in version sequence indicating missing transactions".to_string(),
                ),
            });
        }

        let summary = format!(
            "Analyzed {} transactions across {} batches. Found {} version gaps.",
            total_transactions,
            batch_count,
            version_gaps.len()
        );

        Ok(AnalysisResult {
            analyzer_name: "transaction".to_string(),
            summary,
            metrics,
            details,
        })
    }
}

/// Analyzes pool discovery patterns and DEX distribution
pub struct PoolAnalyzer;

impl Default for PoolAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl DataAnalyzer for PoolAnalyzer {
    fn name(&self) -> &'static str {
        "pool"
    }

    fn analyze(&self, data: &RecordingData) -> Result<AnalysisResult> {
        let mut metrics = HashMap::new();
        let mut details = Vec::new();

        let total_pool_states = data.total_pool_states();

        // Count pools by DEX
        let mut dex_counts = HashMap::new();
        let mut pool_types = HashMap::new();
        let mut unique_pools = std::collections::HashSet::new();

        for batch in &data.batches {
            for pool_state in &batch.pool_initializations {
                *dex_counts.entry(pool_state.dex_name.clone()).or_insert(0) += 1;
                *pool_types.entry(pool_state.pool_type.clone()).or_insert(0) += 1;
                unique_pools.insert(pool_state.pool_id.clone());
            }
        }

        metrics.insert(
            "total_pool_initializations".to_string(),
            serde_json::json!(total_pool_states),
        );
        metrics.insert(
            "unique_pools".to_string(),
            serde_json::json!(unique_pools.len()),
        );
        metrics.insert("dex_breakdown".to_string(), serde_json::json!(dex_counts));
        metrics.insert(
            "pool_type_breakdown".to_string(),
            serde_json::json!(pool_types),
        );

        // Add details for each DEX
        for (dex, count) in &dex_counts {
            details.push(AnalysisDetail {
                category: "dex_distribution".to_string(),
                name: dex.clone(),
                value: serde_json::json!(count),
                description: Some(format!("Pool initializations from {} DEX", dex)),
            });
        }

        // Pool discovery rate
        if let Some(duration_secs) = data.duration_seconds() {
            if duration_secs > 0.0 {
                let discovery_rate = unique_pools.len() as f64 / (duration_secs / 60.0);
                metrics.insert(
                    "pools_per_minute".to_string(),
                    serde_json::json!(discovery_rate),
                );
                details.push(AnalysisDetail {
                    category: "discovery_rate".to_string(),
                    name: "pools_per_minute".to_string(),
                    value: serde_json::json!(format!("{:.2} pools/minute", discovery_rate)),
                    description: Some("Pool discovery rate".to_string()),
                });
            }
        }

        let summary = format!(
            "Found {} unique pools with {} total initializations across {} DEXes",
            unique_pools.len(),
            total_pool_states,
            dex_counts.len()
        );

        Ok(AnalysisResult {
            analyzer_name: "pool".to_string(),
            summary,
            metrics,
            details,
        })
    }
}

/// Analyzes DEX-specific statistics and activity patterns
pub struct DEXAnalyzer;

impl Default for DEXAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl DEXAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl DataAnalyzer for DEXAnalyzer {
    fn name(&self) -> &'static str {
        "dex"
    }

    fn analyze(&self, data: &RecordingData) -> Result<AnalysisResult> {
        let mut metrics = HashMap::new();
        let mut details = Vec::new();

        let mut dex_activity = HashMap::new();

        for batch in &data.batches {
            for pool_state in &batch.pool_initializations {
                let entry = dex_activity
                    .entry(pool_state.dex_name.clone())
                    .or_insert_with(|| {
                        serde_json::json!({
                            "pool_count": 0,
                            "first_seen": batch.timestamp_ms,
                            "last_seen": batch.timestamp_ms,
                            "pool_types": {}
                        })
                    });

                entry["pool_count"] =
                    serde_json::json!(entry["pool_count"].as_u64().unwrap_or(0) + 1);
                entry["last_seen"] = serde_json::json!(batch.timestamp_ms);

                if let Some(pool_types) = entry["pool_types"].as_object_mut() {
                    let count = pool_types
                        .get(&pool_state.pool_type)
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    pool_types.insert(pool_state.pool_type.clone(), serde_json::json!(count + 1));
                }
            }
        }

        metrics.insert("dex_activity".to_string(), serde_json::json!(dex_activity));

        for (dex_name, activity) in &dex_activity {
            details.push(AnalysisDetail {
                category: "dex_analysis".to_string(),
                name: dex_name.clone(),
                value: activity.clone(),
                description: Some(format!(
                    "Comprehensive activity analysis for {} DEX",
                    dex_name
                )),
            });
        }

        let summary = format!(
            "Analyzed activity patterns for {} DEX protocols",
            dex_activity.len()
        );

        Ok(AnalysisResult {
            analyzer_name: "dex".to_string(),
            summary,
            metrics,
            details,
        })
    }
}

/// Analyzes time gaps, recording duration, and batch timing
pub struct TimeRangeAnalyzer;

impl Default for TimeRangeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeRangeAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl DataAnalyzer for TimeRangeAnalyzer {
    fn name(&self) -> &'static str {
        "timerange"
    }

    fn analyze(&self, data: &RecordingData) -> Result<AnalysisResult> {
        let mut metrics = HashMap::new();
        let mut details = Vec::new();

        if let Some((start_time, end_time)) = data.time_range() {
            let duration = end_time - start_time;
            let duration_secs = duration.num_seconds();
            let duration_hours = duration_secs as f64 / 3600.0;

            metrics.insert(
                "recording_start".to_string(),
                serde_json::json!(start_time.to_rfc3339()),
            );
            metrics.insert(
                "recording_end".to_string(),
                serde_json::json!(end_time.to_rfc3339()),
            );
            metrics.insert(
                "duration_seconds".to_string(),
                serde_json::json!(duration_secs),
            );
            metrics.insert(
                "duration_hours".to_string(),
                serde_json::json!(duration_hours),
            );

            details.push(AnalysisDetail {
                category: "time_analysis".to_string(),
                name: "recording_duration".to_string(),
                value: serde_json::json!(format!("{:.2} hours", duration_hours)),
                description: Some("Total recording duration".to_string()),
            });

            // Analyze time gaps between batches
            let mut time_gaps = Vec::new();
            for window in data.batches.windows(2) {
                let gap_ms = window[1].timestamp_ms - window[0].timestamp_ms;
                if gap_ms > 60000 {
                    // Gaps > 1 minute
                    time_gaps.push(gap_ms);
                }
            }

            metrics.insert(
                "large_time_gaps_count".to_string(),
                serde_json::json!(time_gaps.len()),
            );
            if !time_gaps.is_empty() {
                details.push(AnalysisDetail {
                    category: "time_gaps".to_string(),
                    name: "large_gaps".to_string(),
                    value: serde_json::json!(time_gaps),
                    description: Some(
                        "Time gaps larger than 1 minute between batches (ms)".to_string(),
                    ),
                });
            }
        }

        let summary = if let Some((start, end)) = data.time_range() {
            let duration_hours = (end - start).num_seconds() as f64 / 3600.0;
            format!(
                "Recording spans {:.2} hours from {} to {}",
                duration_hours,
                start.format("%Y-%m-%d %H:%M:%S"),
                end.format("%Y-%m-%d %H:%M:%S")
            )
        } else {
            "No timestamp data available for analysis".to_string()
        };

        Ok(AnalysisResult {
            analyzer_name: "timerange".to_string(),
            summary,
            metrics,
            details,
        })
    }
}

/// Analyzes data quality, missing data, and consistency
pub struct DataQualityAnalyzer;

impl Default for DataQualityAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl DataQualityAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl DataAnalyzer for DataQualityAnalyzer {
    fn name(&self) -> &'static str {
        "quality"
    }

    fn analyze(&self, data: &RecordingData) -> Result<AnalysisResult> {
        let mut metrics = HashMap::new();
        let mut details = Vec::new();

        // Check for empty batches
        let empty_batches = data
            .batches
            .iter()
            .enumerate()
            .filter(|(_, batch)| batch.transactions.is_empty())
            .count();

        metrics.insert(
            "empty_batches_count".to_string(),
            serde_json::json!(empty_batches),
        );

        // Check for batches without pool states (might indicate missing pool discovery)
        let batches_without_pools = data
            .batches
            .iter()
            .enumerate()
            .filter(|(_, batch)| batch.pool_initializations.is_empty())
            .count();

        metrics.insert(
            "batches_without_pools".to_string(),
            serde_json::json!(batches_without_pools),
        );

        // Check timestamp consistency (non-decreasing)
        let mut timestamp_issues = 0;
        for window in data.batches.windows(2) {
            if window[1].timestamp_ms < window[0].timestamp_ms {
                timestamp_issues += 1;
            }
        }

        metrics.insert(
            "timestamp_inconsistencies".to_string(),
            serde_json::json!(timestamp_issues),
        );

        // Check for incomplete pool state data
        let mut incomplete_pool_states = 0;
        for batch in &data.batches {
            for pool_state in &batch.pool_initializations {
                if pool_state.pool_id.is_empty()
                    || pool_state.dex_name.is_empty()
                    || pool_state.token_a.is_empty()
                    || pool_state.token_b.is_empty()
                {
                    incomplete_pool_states += 1;
                }
            }
        }

        metrics.insert(
            "incomplete_pool_states".to_string(),
            serde_json::json!(incomplete_pool_states),
        );

        // Quality score calculation (0-100)
        let total_issues = empty_batches + timestamp_issues + incomplete_pool_states;
        let total_items = data.batches.len() + data.total_pool_states();
        let quality_score = if total_items > 0 {
            100.0 * (1.0 - (total_issues as f64 / total_items as f64))
        } else {
            100.0
        };

        metrics.insert(
            "quality_score".to_string(),
            serde_json::json!(quality_score),
        );

        details.push(AnalysisDetail {
            category: "quality_assessment".to_string(),
            name: "overall_quality".to_string(),
            value: serde_json::json!(format!("{:.1}%", quality_score)),
            description: Some("Overall data quality score".to_string()),
        });

        if total_issues > 0 {
            details.push(AnalysisDetail {
                category: "quality_issues".to_string(),
                name: "issue_breakdown".to_string(),
                value: serde_json::json!({
                    "empty_batches": empty_batches,
                    "timestamp_issues": timestamp_issues,
                    "incomplete_pool_states": incomplete_pool_states
                }),
                description: Some("Breakdown of data quality issues found".to_string()),
            });
        }

        let summary = format!(
            "Data quality score: {:.1}%. Found {} issues across {} items analyzed.",
            quality_score, total_issues, total_items
        );

        Ok(AnalysisResult {
            analyzer_name: "quality".to_string(),
            summary,
            metrics,
            details,
        })
    }
}

// Exporters

/// JSON exporter for analysis results
pub struct JsonExporter;

impl Default for JsonExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonExporter {
    pub fn new() -> Self {
        Self
    }
}

impl DataExporter for JsonExporter {
    fn format(&self) -> &'static str {
        "json"
    }

    fn export(&self, results: &[AnalysisResult], output: &Path) -> Result<()> {
        let json_data = serde_json::json!({
            "analysis_timestamp": chrono::Utc::now().to_rfc3339(),
            "analyzer_count": results.len(),
            "results": results
        });

        let json_content = to_string_pretty(&json_data)?;
        fs::write(output, json_content)
            .with_context(|| format!("Failed to write JSON export to {}", output.display()))?;

        info!("Exported analysis results to JSON: {}", output.display());
        Ok(())
    }
}

/// CSV exporter for analysis results
pub struct CsvExporter;

impl Default for CsvExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvExporter {
    pub fn new() -> Self {
        Self
    }
}

impl DataExporter for CsvExporter {
    fn format(&self) -> &'static str {
        "csv"
    }

    fn export(&self, results: &[AnalysisResult], output: &Path) -> Result<()> {
        let mut file = fs::File::create(output)
            .with_context(|| format!("Failed to create CSV file: {}", output.display()))?;

        // Write header
        writeln!(file, "Analyzer,Category,Name,Value,Description")?;

        // Write data rows
        for result in results {
            for detail in &result.details {
                let value_str = match &detail.value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => detail.value.to_string(),
                };

                writeln!(
                    file,
                    "{},{},{},{},{}",
                    result.analyzer_name,
                    detail.category,
                    detail.name,
                    csv_escape(&value_str),
                    csv_escape(detail.description.as_deref().unwrap_or(""))
                )?;
            }
        }

        info!("Exported analysis results to CSV: {}", output.display());
        Ok(())
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let inspector = DataInspector::new();

    match args.command {
        Commands::Analyze {
            input,
            analyzers,
            export_json,
            export_csv,
            output_dir,
            report,
        } => handle_analyze_command(
            &inspector,
            input,
            analyzers,
            export_json,
            export_csv,
            output_dir,
            report,
        ),
        Commands::Pools {
            input,
            dex,
            pool_type,
            time_range,
            export,
        } => handle_pools_command(&inspector, input, dex, pool_type, time_range, export),
        Commands::Timeline {
            input,
            granularity,
            metrics,
            chart,
            export,
        } => handle_timeline_command(&inspector, input, granularity, metrics, chart, export),
        Commands::Quality {
            input,
            checks,
            report,
            detailed,
        } => handle_quality_command(&inspector, input, checks, report, detailed),
    }
}

fn handle_analyze_command(
    inspector: &DataInspector,
    input: PathBuf,
    analyzers: Option<String>,
    export_json: bool,
    export_csv: bool,
    output_dir: PathBuf,
    report: Option<PathBuf>,
) -> Result<()> {
    info!("Loading recording data from: {}", input.display());
    let data = RecordingData::load_from_file(&input)?;

    // Determine which analyzers to run
    let analyzer_names = if let Some(analyzer_list) = analyzers {
        analyzer_list
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        inspector.get_available_analyzers()
    };

    info!("Running analyzers: {:?}", analyzer_names);
    let results = inspector.run_analysis(&data, &analyzer_names)?;

    // Create output directory
    fs::create_dir_all(&output_dir)?;

    // Print summary to console
    println!("\n=== Analysis Results ===");
    for result in &results {
        println!(
            "\n{}: {}",
            result.analyzer_name.to_uppercase(),
            result.summary
        );
    }

    // Export results if requested
    if export_json {
        let json_path = output_dir.join("analysis_results.json");
        inspector.export_results(&results, "json", &json_path)?;
    }

    if export_csv {
        let csv_path = output_dir.join("analysis_results.csv");
        inspector.export_results(&results, "csv", &csv_path)?;
    }

    // Generate summary report if requested
    if let Some(report_path) = report {
        generate_summary_report(&results, &data, &report_path)?;
    }

    Ok(())
}

fn handle_pools_command(
    inspector: &DataInspector,
    input: PathBuf,
    dex: Option<String>,
    _pool_type: Option<String>,
    _time_range: Option<String>,
    export: Option<PathBuf>,
) -> Result<()> {
    info!("Running pool-specific analysis on: {}", input.display());
    let data = RecordingData::load_from_file(&input)?;

    // Run pool analyzer
    let results = inspector.run_analysis(&data, &["pool".to_string()])?;

    // Apply filters and display results
    // TODO: Implement filtering by dex, pool_type, and time_range

    for result in &results {
        println!(
            "\n{}: {}",
            result.analyzer_name.to_uppercase(),
            result.summary
        );

        // Show detailed breakdown
        for detail in &result.details {
            if detail.category == "dex_distribution" {
                if let Some(ref filter_dex) = dex {
                    if detail.name == *filter_dex {
                        println!("  {}: {}", detail.name, detail.value);
                    }
                } else {
                    println!("  {}: {}", detail.name, detail.value);
                }
            }
        }
    }

    if let Some(export_path) = export {
        inspector.export_results(&results, "csv", &export_path)?;
    }

    Ok(())
}

fn handle_timeline_command(
    inspector: &DataInspector,
    input: PathBuf,
    _granularity: String,
    _metrics: String,
    _chart: Option<PathBuf>,
    export: Option<PathBuf>,
) -> Result<()> {
    info!("Running timeline analysis on: {}", input.display());
    let data = RecordingData::load_from_file(&input)?;

    // Run time range analyzer
    let results = inspector.run_analysis(&data, &["timerange".to_string()])?;

    for result in &results {
        println!(
            "\n{}: {}",
            result.analyzer_name.to_uppercase(),
            result.summary
        );
    }

    // TODO: Implement granularity-based timeline analysis
    // TODO: Implement chart generation

    if let Some(export_path) = export {
        inspector.export_results(&results, "csv", &export_path)?;
    }

    Ok(())
}

fn handle_quality_command(
    inspector: &DataInspector,
    input: PathBuf,
    _checks: String,
    report: Option<PathBuf>,
    detailed: bool,
) -> Result<()> {
    info!("Running data quality analysis on: {}", input.display());
    let data = RecordingData::load_from_file(&input)?;

    // Run quality analyzer
    let results = inspector.run_analysis(&data, &["quality".to_string()])?;

    for result in &results {
        println!(
            "\n{}: {}",
            result.analyzer_name.to_uppercase(),
            result.summary
        );

        if detailed {
            for detail in &result.details {
                println!("  {}/{}: {}", detail.category, detail.name, detail.value);
                if let Some(ref desc) = detail.description {
                    println!("    {}", desc);
                }
            }
        }
    }

    if let Some(report_path) = report {
        inspector.export_results(&results, "json", &report_path)?;
    }

    Ok(())
}

fn generate_summary_report(
    results: &[AnalysisResult],
    data: &RecordingData,
    report_path: &PathBuf,
) -> Result<()> {
    let summary_data = serde_json::json!({
        "summary": {
            "file_info": {
                "file_size_bytes": data.file_size,
                "recording_duration": data.duration_seconds().map(|s| format!("{:.1} seconds", s)).unwrap_or("unknown".to_string()),
                "batch_count": data.batches.len(),
                "transaction_count": data.total_transactions(),
                "pool_states_count": data.total_pool_states()
            },
            "analysis_results": results.iter().map(|r| {
                serde_json::json!({
                    "analyzer": r.analyzer_name,
                    "summary": r.summary,
                    "key_metrics": r.metrics
                })
            }).collect::<Vec<_>>()
        },
        "generated_at": chrono::Utc::now().to_rfc3339()
    });

    let json_content = to_string_pretty(&summary_data)?;
    fs::write(report_path, json_content).with_context(|| {
        format!(
            "Failed to write summary report to {}",
            report_path.display()
        )
    })?;

    info!("Generated summary report: {}", report_path.display());
    Ok(())
}
