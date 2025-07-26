# Implementation Plan: Task 8 Performance Monitoring Gaps

## Overview
This plan addresses Task 8 requirements that are not yet implemented in the MDI codebase. The existing monitoring infrastructure is comprehensive, so this focuses on specific enhancements.

## Part 1: Pipeline Stage-Specific Timing

### 1.1 Extend RecordingStats Structure
**File to modify:** `crates/market-data-ingestor/src/recording_monitor.rs`

**Changes needed:**
- Add stage-specific timing fields to `RecordingStats`:
  - `event_extraction_time_ms: f64`
  - `parsing_time_ms: f64` 
  - `filtering_time_ms: f64`
  - `detector_push_time_ms: f64`
  - `stage_timings_collected: u64`

- Add method `record_stage_timing()` that:
  - Updates rolling averages for each stage
  - Tracks total stage timing collections
  - Calculates stage timing percentages of total processing time

### 1.2 Add Stage Timing to Prometheus Metrics
**File to modify:** `crates/market-data-ingestor/src/monitoring.rs`

**Changes needed:**
- Add stage-specific histograms to `PrometheusMetrics`:
  - `event_extraction_latency: Histogram`
  - `parsing_latency: Histogram`
  - `filtering_latency: Histogram`
  - `detector_push_latency: Histogram`

- Update `PrometheusMetrics::new()` to register stage histograms
- Add stage timing update methods to track individual stage performance
- Include stage timings in `ProductionMetrics` struct

### 1.3 Instrument Processing Pipeline
**File to modify:** `crates/market-data-ingestor/src/processor.rs`

**Changes needed:**
- Add timing instrumentation around each pipeline stage in `run_processor()`:
  - Time `event_extractor.process_transaction()` call
  - Time `parser.process_events()` call
  - Time `filter_step.apply()` call
  - Time `detector_push.push()` calls

- Update `update_live_metrics()` to accept stage timings
- Log stage timing breakdowns when total processing exceeds thresholds

## Part 2: Active Warning System for Threshold Violations

### 2.1 Create Warning Detection System
**File to modify:** `crates/market-data-ingestor/src/monitoring.rs`

**Changes needed:**
- Add warning-related fields to `ProductionMetrics`:
  - `latency_warnings_total: u64`
  - `threshold_violations_per_minute: f64`
  - `current_warning_level: WarningLevel`

- Create `WarningLevel` enum: `None`, `Low`, `Medium`, `High`, `Critical`
- Add warning counters to `PrometheusMetrics`

### 2.2 Implement Threshold Monitoring
**File to modify:** `crates/market-data-ingestor/src/processor.rs`

**Changes needed:**
- Add threshold checking logic in `update_live_metrics()`:
  - Compare processing time against `latency_warning_threshold_ms`
  - Generate warnings for consecutive threshold violations
  - Escalate warning levels based on violation frequency

- Add structured logging for threshold violations:
  - Log warning level, current latency, threshold value
  - Include stage timing breakdown in warning logs
  - Rate-limit warning logs to prevent spam

### 2.3 Warning Configuration Enhancement
**File to modify:** `crates/config/src/lib.rs`

**Changes needed:**
- Add warning-specific configuration to `PerformanceConfig`:
  - `warning_escalation_count: u32` (violations before escalation)
  - `critical_latency_multiplier: f64` (factor of threshold for critical)
  - `warning_log_interval_seconds: u64` (rate limiting)

## Part 3: Queue Depth Tracking

### 3.1 Channel Monitoring Infrastructure
**File to modify:** `crates/market-data-ingestor/src/processor.rs`

**Changes needed:**
- Track channel utilization metrics:
  - Monitor `mpsc::channel` capacity vs usage
  - Calculate queue depth percentage
  - Detect backpressure conditions

- Add queue metrics to live stats:
  - `queue_depth_current: usize`
  - `queue_depth_max_observed: usize`
  - `queue_saturation_events: u64`
  - `backpressure_duration_ms: f64`

### 3.2 Queue Metrics in Prometheus
**File to modify:** `crates/market-data-ingestor/src/monitoring.rs`

**Changes needed:**
- Add queue-specific metrics to `PrometheusMetrics`:
  - `queue_depth_current: Gauge`
  - `queue_depth_max: Gauge`
  - `queue_saturation_total: Counter`
  - `backpressure_duration: Histogram`

- Include queue metrics in HTTP endpoint and dashboard

### 3.3 Backpressure Detection
**File to modify:** `crates/market-data-ingestor/src/steps/detector_push.rs`

**Changes needed:**
- Add queue monitoring to `DetectorPushStep`:
  - Track send operation timing
  - Detect when sends are blocking due to full queues
  - Report queue utilization back to metrics system

## Part 4: Comprehensive Unit Tests

### 4.1 Metrics Collection Tests
**New file:** `crates/market-data-ingestor/tests/metrics_collection_tests.rs`

**Test categories needed:**
- Stage timing accuracy tests
- Prometheus metric consistency validation
- Rolling average calculation verification
- Memory usage of metrics collection
- Concurrent metrics update safety

### 4.2 Threshold Detection Tests
**New file:** `crates/market-data-ingestor/tests/threshold_detection_tests.rs`

**Test categories needed:**
- Warning escalation logic verification
- Threshold violation detection accuracy
- Warning rate limiting functionality
- Configuration edge cases
- Warning level transitions

### 4.3 Queue Metrics Tests
**New file:** `crates/market-data-ingestor/tests/queue_metrics_tests.rs`

**Test categories needed:**
- Queue depth calculation accuracy
- Backpressure detection reliability
- Queue saturation event counting
- Performance impact of queue monitoring

## Part 5: Implementation Guidelines

### 5.1 Development Order
1. Implement stage timing infrastructure first (foundation)
2. Add Prometheus metrics for new timing data
3. Implement warning system with basic threshold detection
4. Add queue depth monitoring
5. Write comprehensive tests for all new functionality
6. Performance test to ensure minimal overhead

### 5.2 Performance Requirements
- Stage timing overhead must be < 0.1ms per measurement
- Warning system should not impact processing throughput
- Queue monitoring overhead < 0.05ms per operation
- Memory usage increase < 2% for all new metrics

### 5.3 Error Handling Strategy
- All timing measurements must be non-blocking
- Metrics collection failures should not affect processing
- Warning system failures should be logged but not halt processing
- Queue monitoring should gracefully handle channel state changes

### 5.4 Testing Strategy
- Use property-based testing for timing accuracy
- Mock channel implementations for queue testing
- Benchmark performance impact of new instrumentation
- Integration tests with realistic data loads

### 5.5 Production Readiness Checklist
- All new metrics follow Prometheus naming conventions
- Warning logs are structured and parseable
- Configuration validation for all new settings
- Graceful degradation when monitoring fails
- Documentation updates for new metrics and warnings

## Part 6: Validation Criteria

### 6.1 Functional Requirements
- Stage timings sum to within 5% of total processing time
- Warning system accurately detects threshold violations
- Queue depth metrics reflect actual channel utilization
- All metrics available via Prometheus endpoint

### 6.2 Performance Requirements
- No measurable impact on processing throughput
- Memory usage increase stays under 2%
- Warning system responds within 100ms of violations
- Queue metrics update in real-time

### 6.3 Quality Requirements
- Test coverage > 95% for all new code
- Zero performance regressions in existing functionality
- All edge cases handled gracefully
- Production deployment without configuration changes required
