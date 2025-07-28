use crate::data_source::{DataSource as SourceTrait, FileSource, GrpcSource};
use crate::ingestor_config::IndexerProcessorConfig;
use crate::monitoring::MetricsCollector;
use crate::recording_monitor::{RecordingStats, StageTimings};
use crate::monitoring::WarningLevel;
use crate::steps::{DetectorPushStep, EventExtractorStep, FilterStep, Parser};
use crate::{http_server, TimestampedEvent};
use anyhow::Result;
use chrono::Utc;
use common::types::DetectorMessage;
use config_lib::DataSourceConfig;
use dex_adapters::DexAdapter;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

pub struct MarketDataIngestorProcessor {
    config: IndexerProcessorConfig,
    parser: Parser,
    filter_step: FilterStep,
    update_sender: Option<mpsc::Sender<DetectorMessage>>,
    shutdown_rx: Option<oneshot::Receiver<()>>,
    live_stats: RecordingStats,
    metrics_collector: Arc<RwLock<MetricsCollector>>,
    start_time: Instant,
    
    // Warning system state
    consecutive_violations: u64,
    last_warning_log_time: Option<Instant>,
}

impl MarketDataIngestorProcessor {
    pub async fn new(
        config: IndexerProcessorConfig,
        adapters: HashMap<String, Arc<dyn DexAdapter>>,
    ) -> Result<Self> {
        let parser = Parser::new(adapters);
        let filter_step = FilterStep::from_ingestor_config(&config.ingestor_config.filters);
        let metrics_collector = Arc::new(RwLock::new(MetricsCollector::new()?));

        Ok(Self {
            config,
            parser,
            filter_step,
            update_sender: None,
            shutdown_rx: None,
            live_stats: RecordingStats::new(),
            metrics_collector,
            start_time: Instant::now(),
            consecutive_violations: 0,
            last_warning_log_time: None,
        })
    }

    /// Set the channel sender for pushing updates to the detector
    /// Set the channel sender for pushing detector messages (BlockStart/Updates/BlockEnd)
    pub fn set_update_sender(&mut self, sender: mpsc::Sender<DetectorMessage>) {
        self.update_sender = Some(sender);
    }

    /// Set the shutdown receiver for graceful shutdown
    pub fn set_shutdown_receiver(&mut self, shutdown_rx: oneshot::Receiver<()>) {
        self.shutdown_rx = Some(shutdown_rx);
    }

    /// Get metrics for external access (dashboard, monitoring)
    pub fn get_metrics(&self) -> crate::monitoring::ProductionMetrics {
        if let Ok(collector) = self.metrics_collector.read() {
            let data_source_type = match &self.config.ingestor_config.data_source {
                DataSourceConfig::Grpc { .. } => "grpc",
                DataSourceConfig::File { .. } => "file",
            };
            let is_connected = true; // Assume connected for this context
            collector.get_production_metrics(data_source_type, is_connected)
        } else {
            // Return default metrics if collector is unavailable
            crate::monitoring::ProductionMetrics {
                data_flow: crate::monitoring::DataFlowMetrics {
                    transactions_processed: 0,
                    blocks_processed: 0,
                    batches_processed: 0,
                    last_block_number: None,
                    bytes_processed: 0,
                    processing_latency_ms: 0.0,
                    throughput_transactions_per_sec: 0.0,
                    throughput_batches_per_sec: 0.0,
                    throughput_mb_per_sec: 0.0,
                    transactions_per_block: 0.0,
                    last_activity_timestamp: None,
                },
                pool_discovery: crate::monitoring::PoolDiscoveryMetrics {
                    pools_discovered: 0,
                    pools_accepted: 0,
                    pools_rejected: 0,
                    pool_states_fetched: 0,
                    pool_fetch_success_rate: 0.0,
                    cache_hit_rate: 0.0,
                    discovery_latency_ms: 0.0,
                    cache_size_current: 0,
                    cache_overflows_total: 0,
                    cache_forced_evictions_total: 0,
                    max_cache_size_configured: 0,
                    cache_retention_seconds_configured: 0,
                },
                system_health: crate::monitoring::SystemHealthMetrics {
                    memory_usage_mb: 0.0,
                    memory_usage_percent: 0.0,
                    cpu_usage_percent: 0.0,
                    uptime_seconds: 0,
                    active_connections: 0,
                    disk_usage_mb: 0.0,
                },
                error_tracking: crate::monitoring::ErrorTrackingMetrics {
                    connection_errors: 0,
                    parsing_errors: 0,
                    write_errors: 0,
                    timeout_errors: 0,
                    total_errors: 0,
                    error_rate_percent: 0.0,
                    errors_per_minute: 0.0,
                },
                connection_status: crate::monitoring::ConnectionStatusMetrics {
                    is_connected: false,
                    connection_uptime_seconds: 0,
                    reconnection_count: 0,
                    data_source_type: "unknown".to_string(),
                    last_heartbeat: None,
                    connection_quality: crate::monitoring::ConnectionQuality::Disconnected,
                },
                filter_effectiveness: crate::monitoring::FilterEffectivenessMetrics {
                    updates_received_total: 0,
                    updates_after_filtering: 0,
                    updates_filtered_out: 0,
                    filter_pass_rate_percent: 0.0,
                    filter_processing_time_ms: 0.0,
                    filters_applied_total: 0,
                    filtered_by_token: 0,
                    filtered_by_dex: 0,
                    filtered_by_liquidity: 0,
                    filtered_by_token_pairs: 0,
                },
                stage_timing: crate::monitoring::StageTimingMetrics {
                    event_extraction_time_ms: 0.0,
                    parsing_time_ms: 0.0,
                    filtering_time_ms: 0.0,
                    detector_push_time_ms: 0.0,
                    stage_timings_collected: 0,
                    total_stage_time_ms: 0.0,
                },
                performance_warnings: crate::monitoring::PerformanceWarningMetrics {
                    latency_warnings_total: 0,
                    threshold_violations_per_minute: 0.0,
                    current_warning_level: WarningLevel::None,
                    consecutive_violations: 0,
                    last_warning_timestamp: None,
                    warning_escalation_count: 0,
                },
                queue_monitoring: crate::monitoring::QueueMonitoringMetrics {
                    queue_depth_current: 0,
                    queue_depth_max_observed: 0,
                    queue_saturation_events: 0,
                    queue_saturation_rate_percent: 0.0,
                    backpressure_duration_ms: 0.0,
                    queue_operations_total: 0,
                    average_queue_utilization_percent: 0.0,
                },
            }
        }
    }

    fn update_live_metrics(
        &mut self,
        timestamped_event: &TimestampedEvent,
        processing_time: Duration,
        is_connected: bool,
    ) {
        let transaction = &timestamped_event.raw_event.transaction;
        let metadata = &timestamped_event.raw_event.metadata;

        self.live_stats.transactions_recorded += 1;
        self.live_stats.batches_recorded += 1;

        // Track block numbers properly - count actual blockchain progression
        if let Some(block_height) = metadata.block_height {
            let previous_block = self.live_stats.last_block_number;
            self.live_stats.last_block_number = Some(block_height);

            match previous_block {
                None => {
                    // First block we've seen
                    self.live_stats.blocks_processed = 1;
                }
                Some(prev_block) if block_height > prev_block => {
                    // Calculate how many blocks we've progressed
                    let blocks_progressed = block_height - prev_block;
                    self.live_stats.blocks_processed += blocks_progressed;
                }
                Some(_) => {
                    // Same block or going backwards (shouldn't happen normally)
                    // Don't increment blocks_processed
                }
            }
        } else {
            // If no block height, assume reasonable block size (e.g., every 50 transactions = 1 block)
            if self.live_stats.transactions_recorded % 50 == 1 {
                self.live_stats.blocks_processed += 1;
                // Use version as approximate block indicator
                self.live_stats.last_block_number = Some(transaction.version);
            }
        }

        // Track bytes processed from the event metadata
        if let Some(size_bytes) = metadata.size_bytes {
            self.live_stats.bytes_written += size_bytes;
        }

        let elapsed = self.start_time.elapsed();
        if elapsed.as_secs() > 0 {
            self.live_stats.transactions_per_second =
                self.live_stats.transactions_recorded as f64 / elapsed.as_secs_f64();
            self.live_stats.batches_per_second =
                self.live_stats.batches_recorded as f64 / elapsed.as_secs_f64();
            self.live_stats.bytes_per_second =
                self.live_stats.bytes_written as f64 / elapsed.as_secs_f64();
        }

        // Update processing latency - use microsecond precision to avoid rounding to 0
        let current_latency_ms = processing_time.as_micros() as f64 / 1000.0;
        let total_processed = self.live_stats.transactions_recorded;
        if total_processed == 1 {
            self.live_stats.avg_batch_processing_ms = current_latency_ms;
        } else {
            // Rolling average with better precision
            self.live_stats.avg_batch_processing_ms = (self.live_stats.avg_batch_processing_ms
                * (total_processed - 1) as f64
                + current_latency_ms)
                / total_processed as f64;
        }

        self.live_stats.last_batch_time = Some(SystemTime::now());

        // Performance warning threshold checking
        self.check_performance_thresholds(current_latency_ms);

        let data_source_type = match &self.config.ingestor_config.data_source {
            DataSourceConfig::Grpc { .. } => "grpc",
            DataSourceConfig::File { .. } => "file",
        };

        if let Ok(mut collector) = self.metrics_collector.write() {
            collector.update_metrics(&self.live_stats, data_source_type, is_connected);
        }
    }

    /// Check performance thresholds and trigger warnings with rate limiting and escalation
    fn check_performance_thresholds(&mut self, current_latency_ms: f64) {
        let performance_config = &self.config.ingestor_config.performance;
        let threshold_ms = performance_config.latency_warning_threshold_ms as f64;
        let critical_multiplier = performance_config.critical_latency_multiplier;
        let log_interval = Duration::from_secs(performance_config.warning_log_interval_seconds);
        
        if current_latency_ms > threshold_ms {
            // Threshold violation detected
            self.consecutive_violations += 1;
            
            // Calculate warning level based on current state
            let warning_level = self.live_stats.calculate_warning_level(threshold_ms, critical_multiplier);
            
            // Record warning in stats
            self.live_stats.record_performance_warning(warning_level.clone(), threshold_ms);
            
            // Update Prometheus warning metrics
            if let Ok(collector) = self.metrics_collector.read() {
                let escalated = self.consecutive_violations > performance_config.warning_escalation_count as u64;
                collector.update_warning_metrics(&warning_level, self.consecutive_violations, escalated);
            }
            
            // Rate-limited structured logging
            let now = Instant::now();
            let should_log = self.last_warning_log_time
                .map_or(true, |last| now.duration_since(last) >= log_interval);
                
            if should_log {
                self.last_warning_log_time = Some(now);
                
                warn!(
                    latency_ms = current_latency_ms,
                    threshold_ms = threshold_ms,
                    consecutive_violations = self.consecutive_violations,
                    warning_level = ?warning_level,
                    stage_breakdown = ?format!(
                        "extraction={:.2}ms, parsing={:.2}ms, filtering={:.2}ms, detector_push={:.2}ms",
                        self.live_stats.event_extraction_time_ms,
                        self.live_stats.parsing_time_ms,
                        self.live_stats.filtering_time_ms,
                        self.live_stats.detector_push_time_ms
                    ),
                    "Performance threshold violation detected"
                );
            }
        } else {
            // Performance improved - reset violation tracking
            if self.consecutive_violations > 0 {
                info!(
                    latency_ms = current_latency_ms,
                    threshold_ms = threshold_ms,
                    previous_violations = self.consecutive_violations,
                    "Performance improved - resetting violation tracking"
                );
                
                self.consecutive_violations = 0;
                self.live_stats.reset_violation_tracking();
                
                // Update Prometheus metrics
                if let Ok(collector) = self.metrics_collector.read() {
                    collector.reset_warning_metrics();
                }
            }
        }
    }

    pub async fn run_processor(mut self) -> Result<()> {
        info!("Starting Market Data Ingestor processor");

        let update_sender = self
            .update_sender
            .take()
            .ok_or_else(|| anyhow::anyhow!("Update sender not set"))?;

        let mut shutdown_rx = self
            .shutdown_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Shutdown receiver not set"))?;

        let starting_version = self.config.transaction_stream_config.starting_version;
        info!(starting_version = ?starting_version, "Starting from version");

        // Start HTTP metrics server if enabled
        let metrics_server_handle = if self.config.ingestor_config.performance.metrics_enabled {
            let bind_address = format!(
                "127.0.0.1:{}",
                self.config.ingestor_config.performance.metrics_port
            );
            let metrics_collector = self.metrics_collector.clone();

            info!("Starting metrics server on {}", bind_address);

            Some(tokio::spawn(async move {
                info!("Metrics server task started");
                if let Err(e) =
                    http_server::start_metrics_server(bind_address, metrics_collector).await
                {
                    error!("Metrics server error: {}", e);
                } else {
                    info!("Metrics server finished normally");
                }
            }))
        } else {
            info!("Metrics server disabled");
            None
        };

        // Select data source (gRPC live stream or file replay) from config
        let mut source: Box<dyn SourceTrait> = match &self.config.ingestor_config.data_source {
            DataSourceConfig::Grpc {
                endpoint: _,
                timeout_ms: _,
                ..
            } => Box::new(GrpcSource::new(
                self.config.transaction_stream_config.clone(),
            )),
            DataSourceConfig::File {
                path, replay_speed, ..
            } => {
                let speed = replay_speed.unwrap_or(1.0);
                Box::new(FileSource::new(path.clone(), speed))
            }
        };

        // Start the data source
        source
            .start()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start data source: {}", e))?;

        // Create processing steps
        let mut event_extractor =
            EventExtractorStep::from_adapter_configs(self.config.ingestor_config.adapters.clone());

        let detector_push = DetectorPushStep::new(update_sender);

        info!("Starting main processing loop");

        loop {
            tokio::select! {
                biased;

                _ = &mut shutdown_rx => {
                    warn!("Shutdown signal received. Exiting MDI processing loop.");
                    break;
                }

                event_result = source.next_event() => {
                    let processing_start = Instant::now();
                    match event_result {
                        Ok(Some(timestamped_event)) => {
                            let transaction = timestamped_event.raw_event.transaction.clone();
                            let version = transaction.version;

                            info!(
                                version = version,
                                sequence = timestamped_event.sequence,
                                "Received transaction event"
                            );

                            // Use block height for block messages if available, otherwise use version
                            let block_number = timestamped_event.raw_event.metadata.block_height.unwrap_or(version);

                            // Emit BlockStart before processing this transaction with queue monitoring
                            let block_start_metrics = detector_push.push_with_metrics(DetectorMessage::BlockStart {
                                block_number,
                                timestamp: Utc::now(),
                            }).await?;
                            
                            // Record queue metrics for BlockStart
                            if let Err(e) = self.live_stats.record_queue_metrics(
                                block_start_metrics.queue_depth_before,
                                block_start_metrics.operation_time_ms,
                                block_start_metrics.was_blocked,
                            ) {
                                warn!("Failed to record queue metrics: {}", e);
                            }

                            // Extract relevant events with timing
                            let extraction_start = Instant::now();
                            match event_extractor.process_transaction(transaction.clone()).await {
                                Ok(events) if !events.is_empty() => {
                                    let extraction_time = extraction_start.elapsed().as_micros() as f64 / 1000.0;

                                    // Parse events into market updates with timing
                                    let parsing_start = Instant::now();
                                    match self.parser.process_events(&events) {
                                         Ok(mut updates) if !updates.is_empty() => {
                                            let parsing_time = parsing_start.elapsed().as_micros() as f64 / 1000.0;

                                            // Filter in-place to drop unwanted pools with metrics collection
                                            let filtering_start = Instant::now();
                                            let filter_metrics = self.filter_step.apply_with_metrics(&mut updates);
                                            let filtering_time = filtering_start.elapsed().as_micros() as f64 / 1000.0;
                                            
                                            self.live_stats.record_filter_applied(&filter_metrics);

                                            // Log significant filtering events
                                            if filter_metrics.updates_filtered_out > 0 {
                                                info!(
                                                    version = version,
                                                    received = filter_metrics.updates_received_total,
                                                    passed = filter_metrics.updates_after_filtering,
                                                    filtered = filter_metrics.updates_filtered_out,
                                                    pass_rate = filter_metrics.filter_pass_rate_percent,
                                                    processing_time_ms = filter_metrics.filter_processing_time_ms,
                                                    "Filter applied with {} updates filtered out",
                                                    filter_metrics.updates_filtered_out
                                                );
                                            }

                                            let mut detector_push_time = 0.0;
                                            if !updates.is_empty() {
                                                // Push updates to detector with timing and queue monitoring
                                                let detector_push_start = Instant::now();
                                                let mut total_queue_time = 0.0;
                                                let mut max_queue_depth = 0;
                                                let mut any_blocked = false;
                                                
                                                for update in updates {
                                                    match detector_push.push_with_metrics(DetectorMessage::MarketUpdate(update)).await {
                                                        Ok(queue_metrics) => {
                                                            total_queue_time += queue_metrics.operation_time_ms;
                                                            max_queue_depth = max_queue_depth.max(queue_metrics.queue_depth_before);
                                                            any_blocked = any_blocked || queue_metrics.was_blocked;
                                                            
                                                            // Record individual queue operation metrics
                                                            if let Err(e) = self.live_stats.record_queue_metrics(
                                                                queue_metrics.queue_depth_before,
                                                                queue_metrics.operation_time_ms,
                                                                queue_metrics.was_blocked,
                                                            ) {
                                                                warn!("Failed to record queue metrics: {}", e);
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!(version = version, error = %e, "Failed to send MarketUpdate message");
                                                            self.live_stats.write_errors += 1;
                                                        }
                                                    }
                                                }
                                                detector_push_time = detector_push_start.elapsed().as_micros() as f64 / 1000.0;
                                                
                                                // Update metrics collector with queue statistics
                                                if let Ok(collector) = self.metrics_collector.read() {
                                                    collector.update_queue_metrics(
                                                        self.live_stats.queue_depth_current,
                                                        self.live_stats.queue_depth_max_observed,
                                                        if any_blocked { 1 } else { 0 },
                                                        total_queue_time,
                                                    );
                                                }
                                            }

                                            // Record stage timings
                                            let stage_timings = StageTimings::new(
                                                extraction_time,
                                                parsing_time,
                                                filtering_time,
                                                detector_push_time,
                                            );
                                            self.live_stats.record_stage_timing(&stage_timings);

                                            // Update Prometheus stage metrics
                                            if let Ok(collector) = self.metrics_collector.read() {
                                                collector.update_stage_timings(&stage_timings);
                                            }
                                        }
                                        Ok(_) => {
                                            // No updates generated - still record extraction and parsing times
                                            let parsing_time = parsing_start.elapsed().as_micros() as f64 / 1000.0;
                                            let stage_timings = StageTimings::new(extraction_time, parsing_time, 0.0, 0.0);
                                            self.live_stats.record_stage_timing(&stage_timings);
                                        }
                                        Err(e) => {
                                            error!(version = version, error = %e, "Failed to parse events");
                                            self.live_stats.parsing_errors += 1;
                                            // Record partial timing for failed parsing
                                            let parsing_time = parsing_start.elapsed().as_micros() as f64 / 1000.0;
                                            let stage_timings = StageTimings::new(extraction_time, parsing_time, 0.0, 0.0);
                                            self.live_stats.record_stage_timing(&stage_timings);
                                        }
                                    }
                                }
                                Ok(_) => {
                                    // No relevant events - record extraction time only
                                    let extraction_time = extraction_start.elapsed().as_micros() as f64 / 1000.0;
                                    let stage_timings = StageTimings::new(extraction_time, 0.0, 0.0, 0.0);
                                    self.live_stats.record_stage_timing(&stage_timings);
                                }
                                Err(e) => {
                                    error!(version = version, error = %e, "Failed to extract events");
                                    self.live_stats.parsing_errors += 1;
                                    // Record failed extraction time
                                    let extraction_time = extraction_start.elapsed().as_micros() as f64 / 1000.0;
                                    let stage_timings = StageTimings::new(extraction_time, 0.0, 0.0, 0.0);
                                    self.live_stats.record_stage_timing(&stage_timings);
                                }
                            }

                            // Emit BlockEnd after processing this transaction with queue monitoring  
                            let block_end_metrics = detector_push.push_with_metrics(DetectorMessage::BlockEnd { block_number }).await?;
                            
                            // Record queue metrics for BlockEnd
                            if let Err(e) = self.live_stats.record_queue_metrics(
                                block_end_metrics.queue_depth_before,
                                block_end_metrics.operation_time_ms,
                                block_end_metrics.was_blocked,
                            ) {
                                warn!("Failed to record queue metrics: {}", e);
                            }

                            // Update metrics with processing time and connection status
                            let processing_time = processing_start.elapsed();
                            let is_connected = source.is_active();
                            self.update_live_metrics(&timestamped_event, processing_time, is_connected);
                        }
                        Ok(None) => {
                            info!("Data source ended");
                            break;
                        }
                        Err(e) => {
                            error!(error = %e, "Error receiving transaction event");
                            self.live_stats.connection_errors += 1;
                            break;
                        }
                    }
                }
            }
        }

        // Clean up the data source
        if let Err(e) = source.stop().await {
            warn!(error = %e, "Error stopping data source");
        }

        // Cancel metrics server if it was started
        if let Some(handle) = metrics_server_handle {
            handle.abort();
        }

        Ok(())
    }
}
