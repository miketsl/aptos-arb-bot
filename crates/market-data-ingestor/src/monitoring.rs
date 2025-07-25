use prometheus::{Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, Opts, Registry};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::System;

use crate::recording_monitor::RecordingStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionMetrics {
    pub data_flow: DataFlowMetrics,
    pub pool_discovery: PoolDiscoveryMetrics,
    pub system_health: SystemHealthMetrics,
    pub error_tracking: ErrorTrackingMetrics,
    pub connection_status: ConnectionStatusMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFlowMetrics {
    pub transactions_processed: u64,
    pub blocks_processed: u64,
    pub batches_processed: u64,
    pub last_block_number: Option<u64>,
    pub bytes_processed: u64,
    pub processing_latency_ms: f64,
    pub throughput_transactions_per_sec: f64,
    pub throughput_batches_per_sec: f64,
    pub throughput_mb_per_sec: f64,
    pub transactions_per_block: f64,
    pub last_activity_timestamp: Option<SystemTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolDiscoveryMetrics {
    pub pools_discovered: u64,
    pub pools_accepted: u64,
    pub pools_rejected: u64,
    pub pool_states_fetched: u64,
    pub pool_fetch_success_rate: f64,
    pub cache_hit_rate: f64,
    pub discovery_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealthMetrics {
    pub memory_usage_mb: f64,
    pub memory_usage_percent: f64,
    pub cpu_usage_percent: f64,
    pub uptime_seconds: u64,
    pub active_connections: u64,
    pub disk_usage_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorTrackingMetrics {
    pub connection_errors: u64,
    pub parsing_errors: u64,
    pub write_errors: u64,
    pub timeout_errors: u64,
    pub total_errors: u64,
    pub error_rate_percent: f64,
    pub errors_per_minute: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatusMetrics {
    pub is_connected: bool,
    pub connection_uptime_seconds: u64,
    pub reconnection_count: u64,
    pub data_source_type: String,
    pub last_heartbeat: Option<SystemTime>,
    pub connection_quality: ConnectionQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionQuality {
    Excellent,
    Good,
    Fair,
    Poor,
    Disconnected,
}

pub struct PrometheusMetrics {
    pub registry: Registry,

    pub transactions_processed: Counter,
    pub blocks_processed: Counter,
    pub batches_processed: Counter,
    pub bytes_processed: Counter,
    pub processing_latency: Histogram,
    pub last_block_number: Gauge,
    pub transactions_per_block: Gauge,

    pub pools_discovered: Counter,
    pub pools_accepted: Counter,
    pub pools_rejected: Counter,
    pub pool_fetch_success_rate: Gauge,
    pub cache_hit_rate: Gauge,

    pub memory_usage: Gauge,
    pub cpu_usage: Gauge,
    pub connection_uptime: Gauge,
    pub active_connections: Gauge,

    pub error_counter: CounterVec,
    pub error_rate: Gauge,

    pub connection_status: GaugeVec,
    pub throughput: GaugeVec,
}

impl PrometheusMetrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        let transactions_processed = Counter::new(
            "mdi_transactions_processed_total",
            "Total number of transactions processed",
        )?;
        registry.register(Box::new(transactions_processed.clone()))?;

        let batches_processed = Counter::new(
            "mdi_batches_processed_total",
            "Total number of batches processed",
        )?;
        registry.register(Box::new(batches_processed.clone()))?;

        let blocks_processed = Counter::new(
            "mdi_blocks_processed_total",
            "Total number of blocks processed",
        )?;
        registry.register(Box::new(blocks_processed.clone()))?;

        let bytes_processed = Counter::new(
            "mdi_bytes_processed_total",
            "Total number of bytes processed",
        )?;
        registry.register(Box::new(bytes_processed.clone()))?;

        let processing_latency = Histogram::with_opts(HistogramOpts::new(
            "mdi_processing_latency_seconds",
            "Processing latency in seconds",
        ))?;
        registry.register(Box::new(processing_latency.clone()))?;

        let last_block_number = Gauge::new("mdi_last_block_number", "Last processed block number")?;
        registry.register(Box::new(last_block_number.clone()))?;

        let transactions_per_block = Gauge::new(
            "mdi_transactions_per_block",
            "Average transactions per block",
        )?;
        registry.register(Box::new(transactions_per_block.clone()))?;

        let pools_discovered = Counter::new(
            "mdi_pools_discovered_total",
            "Total number of pools discovered",
        )?;
        registry.register(Box::new(pools_discovered.clone()))?;

        let pools_accepted =
            Counter::new("mdi_pools_accepted_total", "Total number of pools accepted")?;
        registry.register(Box::new(pools_accepted.clone()))?;

        let pools_rejected =
            Counter::new("mdi_pools_rejected_total", "Total number of pools rejected")?;
        registry.register(Box::new(pools_rejected.clone()))?;

        let pool_fetch_success_rate =
            Gauge::new("mdi_pool_fetch_success_rate", "Pool fetch success rate")?;
        registry.register(Box::new(pool_fetch_success_rate.clone()))?;

        let cache_hit_rate = Gauge::new("mdi_cache_hit_rate", "Cache hit rate")?;
        registry.register(Box::new(cache_hit_rate.clone()))?;

        let memory_usage = Gauge::new("mdi_memory_usage_bytes", "Memory usage in bytes")?;
        registry.register(Box::new(memory_usage.clone()))?;

        let cpu_usage = Gauge::new("mdi_cpu_usage_percent", "CPU usage percentage")?;
        registry.register(Box::new(cpu_usage.clone()))?;

        let connection_uptime = Gauge::new(
            "mdi_connection_uptime_seconds",
            "Connection uptime in seconds",
        )?;
        registry.register(Box::new(connection_uptime.clone()))?;

        let active_connections =
            Gauge::new("mdi_active_connections", "Number of active connections")?;
        registry.register(Box::new(active_connections.clone()))?;

        let error_counter = CounterVec::new(
            Opts::new("mdi_errors_total", "Total number of errors by type"),
            &["error_type"],
        )?;
        registry.register(Box::new(error_counter.clone()))?;

        let error_rate = Gauge::new("mdi_error_rate_percent", "Error rate percentage")?;
        registry.register(Box::new(error_rate.clone()))?;

        let connection_status = GaugeVec::new(
            Opts::new("mdi_connection_status", "Connection status by type"),
            &["data_source_type", "status"],
        )?;
        registry.register(Box::new(connection_status.clone()))?;

        let throughput = GaugeVec::new(
            Opts::new("mdi_throughput", "Throughput metrics by type"),
            &["metric_type"],
        )?;
        registry.register(Box::new(throughput.clone()))?;

        Ok(Self {
            registry,
            transactions_processed,
            blocks_processed,
            batches_processed,
            bytes_processed,
            processing_latency,
            last_block_number,
            transactions_per_block,
            pools_discovered,
            pools_accepted,
            pools_rejected,
            pool_fetch_success_rate,
            cache_hit_rate,
            memory_usage,
            cpu_usage,
            connection_uptime,
            active_connections,
            error_counter,
            error_rate,
            connection_status,
            throughput,
        })
    }

    pub fn update_from_recording_stats(&self, stats: &RecordingStats) {
        self.transactions_processed
            .inc_by(stats.transactions_recorded as f64);
        self.blocks_processed.inc_by(stats.blocks_processed as f64);
        self.batches_processed.inc_by(stats.batches_recorded as f64);
        self.bytes_processed.inc_by(stats.bytes_written as f64);

        if let Some(block_number) = stats.last_block_number {
            self.last_block_number.set(block_number as f64);
        }

        let txn_per_block = if stats.blocks_processed > 0 {
            stats.transactions_recorded as f64 / stats.blocks_processed as f64
        } else {
            0.0
        };
        self.transactions_per_block.set(txn_per_block);

        if stats.avg_batch_processing_ms > 0.0 {
            self.processing_latency
                .observe(stats.avg_batch_processing_ms / 1000.0);
        }

        self.pools_discovered.inc_by(stats.pools_discovered as f64);
        self.pools_accepted.inc_by(stats.pools_accepted as f64);
        self.pools_rejected.inc_by(stats.pools_rejected as f64);

        if stats.pools_discovered > 0 {
            let success_rate = stats.pools_accepted as f64 / stats.pools_discovered as f64;
            self.pool_fetch_success_rate.set(success_rate);
        }

        self.error_counter
            .with_label_values(&["connection"])
            .inc_by(stats.connection_errors as f64);
        self.error_counter
            .with_label_values(&["parsing"])
            .inc_by(stats.parsing_errors as f64);
        self.error_counter
            .with_label_values(&["write"])
            .inc_by(stats.write_errors as f64);
        self.error_counter
            .with_label_values(&["timeout"])
            .inc_by(stats.pool_fetch_timeouts as f64);

        let total_operations = stats.batches_recorded + stats.pools_discovered;
        let total_errors = stats.connection_errors
            + stats.parsing_errors
            + stats.write_errors
            + stats.pool_fetch_failures;
        if total_operations > 0 {
            let error_rate = (total_errors as f64 / total_operations as f64) * 100.0;
            self.error_rate.set(error_rate);
        }

        self.throughput
            .with_label_values(&["transactions_per_sec"])
            .set(stats.transactions_per_second);
        self.throughput
            .with_label_values(&["batches_per_sec"])
            .set(stats.batches_per_second);
        self.throughput
            .with_label_values(&["mb_per_sec"])
            .set(stats.bytes_per_second / 1024.0 / 1024.0);
    }

    pub fn update_system_metrics(&self, system: &mut System) {
        system.refresh_all();

        let _memory_total = system.total_memory();
        let memory_used = system.used_memory();

        self.memory_usage.set(memory_used as f64);

        let cpu_usage = system.global_cpu_info().cpu_usage() as f64;
        self.cpu_usage.set(cpu_usage);
    }

    pub fn update_connection_status(
        &self,
        data_source_type: &str,
        is_connected: bool,
        uptime_seconds: u64,
    ) {
        self.connection_status
            .with_label_values(&[data_source_type, "connected"])
            .set(if is_connected { 1.0 } else { 0.0 });

        if is_connected {
            self.connection_uptime.set(uptime_seconds as f64);
            self.active_connections.set(1.0);
        } else {
            self.active_connections.set(0.0);
        }
    }
}

impl Default for PrometheusMetrics {
    fn default() -> Self {
        Self::new().expect("Failed to create default PrometheusMetrics")
    }
}

pub struct MetricsCollector {
    prometheus: PrometheusMetrics,
    system: System,
    start_time: SystemTime,
    last_stats: Option<RecordingStats>,
}

impl MetricsCollector {
    pub fn new() -> Result<Self, prometheus::Error> {
        Ok(Self {
            prometheus: PrometheusMetrics::new()?,
            system: System::new_all(),
            start_time: SystemTime::now(),
            last_stats: None,
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.prometheus.registry
    }

    pub fn update_metrics(
        &mut self,
        stats: &RecordingStats,
        data_source_type: &str,
        is_connected: bool,
    ) {
        self.prometheus.update_from_recording_stats(stats);
        self.prometheus.update_system_metrics(&mut self.system);

        let uptime = self
            .start_time
            .elapsed()
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        self.prometheus
            .update_connection_status(data_source_type, is_connected, uptime);

        self.last_stats = Some(stats.clone());
    }

    pub fn get_production_metrics(
        &self,
        data_source_type: &str,
        is_connected: bool,
    ) -> ProductionMetrics {
        let stats = self.last_stats.as_ref().cloned().unwrap_or_default();

        let uptime = self
            .start_time
            .elapsed()
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        let memory_total = self.system.total_memory();
        let memory_used = self.system.used_memory();
        let memory_percent = if memory_total > 0 {
            (memory_used as f64 / memory_total as f64) * 100.0
        } else {
            0.0
        };

        let total_operations = stats.batches_recorded + stats.pools_discovered;
        let total_errors = stats.connection_errors
            + stats.parsing_errors
            + stats.write_errors
            + stats.pool_fetch_failures;
        let error_rate = if total_operations > 0 {
            (total_errors as f64 / total_operations as f64) * 100.0
        } else {
            0.0
        };

        let pool_success_rate = if stats.pools_discovered > 0 {
            (stats.pools_accepted as f64 / stats.pools_discovered as f64) * 100.0
        } else {
            100.0
        };

        let connection_quality = match (is_connected, error_rate) {
            (false, _) => ConnectionQuality::Disconnected,
            (true, rate) if rate < 1.0 => ConnectionQuality::Excellent,
            (true, rate) if rate < 5.0 => ConnectionQuality::Good,
            (true, rate) if rate < 15.0 => ConnectionQuality::Fair,
            (true, _) => ConnectionQuality::Poor,
        };

        let txn_per_block = if stats.blocks_processed > 0 {
            stats.transactions_recorded as f64 / stats.blocks_processed as f64
        } else {
            0.0
        };

        ProductionMetrics {
            data_flow: DataFlowMetrics {
                transactions_processed: stats.transactions_recorded,
                blocks_processed: stats.blocks_processed,
                batches_processed: stats.batches_recorded,
                last_block_number: stats.last_block_number,
                bytes_processed: stats.bytes_written,
                processing_latency_ms: stats.avg_batch_processing_ms,
                throughput_transactions_per_sec: stats.transactions_per_second,
                throughput_batches_per_sec: stats.batches_per_second,
                throughput_mb_per_sec: stats.bytes_per_second / 1024.0 / 1024.0,
                transactions_per_block: txn_per_block,
                last_activity_timestamp: stats.last_batch_time,
            },
            pool_discovery: PoolDiscoveryMetrics {
                pools_discovered: stats.pools_discovered,
                pools_accepted: stats.pools_accepted,
                pools_rejected: stats.pools_rejected,
                pool_states_fetched: stats.pool_states_fetched,
                pool_fetch_success_rate: pool_success_rate,
                cache_hit_rate: 0.0,       // TODO: Add cache metrics
                discovery_latency_ms: 0.0, // TODO: Add discovery latency tracking
            },
            system_health: SystemHealthMetrics {
                memory_usage_mb: memory_used as f64 / 1024.0 / 1024.0,
                memory_usage_percent: memory_percent,
                cpu_usage_percent: self.system.global_cpu_info().cpu_usage() as f64,
                uptime_seconds: uptime,
                active_connections: if is_connected { 1 } else { 0 },
                disk_usage_mb: 0.0, // TODO: Add disk usage tracking
            },
            error_tracking: ErrorTrackingMetrics {
                connection_errors: stats.connection_errors,
                parsing_errors: stats.parsing_errors,
                write_errors: stats.write_errors,
                timeout_errors: stats.pool_fetch_timeouts,
                total_errors,
                error_rate_percent: error_rate,
                errors_per_minute: 0.0, // TODO: Add error rate calculation
            },
            connection_status: ConnectionStatusMetrics {
                is_connected,
                connection_uptime_seconds: uptime,
                reconnection_count: 0, // TODO: Add reconnection tracking
                data_source_type: data_source_type.to_string(),
                last_heartbeat: stats.last_batch_time,
                connection_quality,
            },
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new().expect("Failed to create default MetricsCollector")
    }
}

pub fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

pub fn timestamp_to_rfc3339(timestamp: Option<SystemTime>) -> String {
    timestamp
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| {
            chrono::DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| "invalid".to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prometheus_metrics_creation() {
        let metrics = PrometheusMetrics::new().expect("Failed to create metrics");
        assert!(!metrics.registry.gather().is_empty());
    }

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new().expect("Failed to create collector");
        assert!(!collector.registry().gather().is_empty());
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m 1s");
    }

    #[test]
    fn test_production_metrics_creation() {
        let mut collector = MetricsCollector::new().expect("Failed to create collector");
        let stats = RecordingStats::new();

        collector.update_metrics(&stats, "grpc", true);
        let metrics = collector.get_production_metrics("grpc", true);

        assert_eq!(metrics.connection_status.data_source_type, "grpc");
        assert!(metrics.connection_status.is_connected);
        assert!(matches!(
            metrics.connection_status.connection_quality,
            ConnectionQuality::Excellent
        ));
    }
}
