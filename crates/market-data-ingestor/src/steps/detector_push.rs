use anyhow::Result;
use common::types::DetectorMessage;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, error, warn, info};

/// Queue operation metrics for a single send operation
#[derive(Debug, Clone)]
pub struct QueueOperationMetrics {
    pub operation_time_ms: f64,
    pub queue_depth_before: usize,
    pub was_blocked: bool,
    pub channel_capacity: usize,
}

impl QueueOperationMetrics {
    pub fn new(operation_time_ms: f64, queue_depth_before: usize, was_blocked: bool, channel_capacity: usize) -> Self {
        Self {
            operation_time_ms,
            queue_depth_before,
            was_blocked,
            channel_capacity,
        }
    }
}

/// Congestion detection and backpressure metrics
#[derive(Debug, Clone)]
pub struct CongestionMetrics {
    pub channel_congestion_events: u64,
    pub backpressure_duration_total_ms: f64,
    pub queue_depth_current: usize,
    pub send_timeout_errors: u64,
    pub circuit_breaker_activations: u64,
    pub last_congestion_event: Option<SystemTime>,
    pub consecutive_failures: u32,
}

impl Default for CongestionMetrics {
    fn default() -> Self {
        Self {
            channel_congestion_events: 0,
            backpressure_duration_total_ms: 0.0,
            queue_depth_current: 0,
            send_timeout_errors: 0,
            circuit_breaker_activations: 0,
            last_congestion_event: None,
            consecutive_failures: 0,
        }
    }
}

/// Circuit breaker states for channel failure handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    Closed,  // Normal operation
    Open,    // Blocking all requests due to failures
    HalfOpen, // Testing recovery
}

/// Configuration for backpressure handling and circuit breaker
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    pub send_timeout: Duration,
    pub circuit_breaker_failure_threshold: u32,
    pub circuit_breaker_recovery_timeout: Duration,
    pub queue_depth_warning_threshold: f64,
    pub retry_attempts: u32,
    pub retry_base_delay: Duration,
    pub retry_max_delay: Duration,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            send_timeout: Duration::from_millis(5000),
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_recovery_timeout: Duration::from_millis(30000),
            queue_depth_warning_threshold: 0.8,
            retry_attempts: 3,
            retry_base_delay: Duration::from_millis(100),
            retry_max_delay: Duration::from_millis(5000),
        }
    }
}

/// Circuit breaker for managing persistent channel failures
#[derive(Debug)]
pub struct CircuitBreaker {
    state: Arc<std::sync::RwLock<CircuitBreakerState>>,
    failure_count: AtomicU32,
    last_failure_time: AtomicU64,
    success_count: AtomicU32,
    config: BackpressureConfig,
}

impl CircuitBreaker {
    fn new(config: BackpressureConfig) -> Self {
        Self {
            state: Arc::new(std::sync::RwLock::new(CircuitBreakerState::Closed)),
            failure_count: AtomicU32::new(0),
            last_failure_time: AtomicU64::new(0),
            success_count: AtomicU32::new(0),
            config,
        }
    }

    fn should_allow_request(&self) -> bool {
        let state = self.state.read().unwrap();
        match *state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let last_failure = self.last_failure_time.load(Ordering::Relaxed);
                
                if now - last_failure >= self.config.circuit_breaker_recovery_timeout.as_millis() as u64 {
                    // Try to transition to half-open
                    drop(state);
                    if let Ok(mut state) = self.state.write() {
                        if *state == CircuitBreakerState::Open {
                            *state = CircuitBreakerState::HalfOpen;
                            info!("Circuit breaker transitioning to half-open state for recovery testing");
                            return true;
                        }
                    }
                }
                false
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    fn record_success(&self) {
        let success_count = self.success_count.fetch_add(1, Ordering::Relaxed);
        
        let state = self.state.read().unwrap();
        if *state == CircuitBreakerState::HalfOpen && success_count >= 2 {
            drop(state);
            if let Ok(mut state) = self.state.write() {
                if *state == CircuitBreakerState::HalfOpen {
                    *state = CircuitBreakerState::Closed;
                    self.failure_count.store(0, Ordering::Relaxed);
                    self.success_count.store(0, Ordering::Relaxed);
                    info!("Circuit breaker recovered to closed state");
                }
            }
        }
    }

    fn record_failure(&self) -> bool {
        let failure_count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        self.last_failure_time.store(now, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);

        if failure_count >= self.config.circuit_breaker_failure_threshold {
            let mut state = self.state.write().unwrap();
            if *state != CircuitBreakerState::Open {
                *state = CircuitBreakerState::Open;
                warn!(
                    failure_count = failure_count,
                    threshold = self.config.circuit_breaker_failure_threshold,
                    "Circuit breaker opened due to consecutive failures"
                );
                return true; // Circuit breaker just opened
            }
        }
        false
    }

    fn get_state(&self) -> CircuitBreakerState {
        *self.state.read().unwrap()
    }
}

/// Step that pushes detector messages (BlockStart/MarketUpdate/BlockEnd) with enterprise-grade error handling.
pub struct DetectorPushStep {
    sender: mpsc::Sender<DetectorMessage>,
    circuit_breaker: CircuitBreaker,
    congestion_metrics: Arc<std::sync::RwLock<CongestionMetrics>>,
    config: BackpressureConfig,
}

impl DetectorPushStep {
    pub fn new(sender: mpsc::Sender<DetectorMessage>, config: BackpressureConfig) -> Self {
        Self {
            sender,
            circuit_breaker: CircuitBreaker::new(config.clone()),
            congestion_metrics: Arc::new(std::sync::RwLock::new(CongestionMetrics::default())),
            config,
        }
    }

    /// Send a single detector message to the receiver.
    pub async fn push(&self, msg: DetectorMessage) -> Result<()> {
        let result = self.push_with_timeout_and_retry(msg).await;
        result.map(|_| ())
    }

    /// Send a detector message with queue monitoring and timing
    pub async fn push_with_metrics(&self, msg: DetectorMessage) -> Result<QueueOperationMetrics> {
        self.push_with_timeout_and_retry(msg).await
    }

    /// Send a detector message with comprehensive timeout, retry, and circuit breaker handling
    pub async fn push_with_timeout_and_retry(&self, msg: DetectorMessage) -> Result<QueueOperationMetrics> {
        let start_time = Instant::now();
        
        // Check circuit breaker first
        if !self.circuit_breaker.should_allow_request() {
            self.record_congestion_event("circuit_breaker_blocked".to_string());
            return Err(anyhow::anyhow!("Circuit breaker is open, blocking request"));
        }

        // Attempt send with retry logic
        let mut last_error = None;
        for attempt in 0..=self.config.retry_attempts {
            let attempt_start = Instant::now();
            
            // Get current channel state for monitoring
            let channel_capacity = self.sender.capacity();
            let queue_depth_before = channel_capacity.saturating_sub(self.sender.capacity());
            
            // Check queue depth warning threshold
            let queue_utilization = queue_depth_before as f64 / channel_capacity as f64;
            if queue_utilization >= self.config.queue_depth_warning_threshold {
                warn!(
                    queue_depth = queue_depth_before,
                    channel_capacity = channel_capacity,
                    utilization_percent = queue_utilization * 100.0,
                    threshold_percent = self.config.queue_depth_warning_threshold * 100.0,
                    "Queue depth approaching warning threshold"
                );
                self.record_congestion_event("queue_depth_warning".to_string());
            }

            debug!(
                ?msg, 
                queue_depth = queue_depth_before, 
                attempt = attempt + 1,
                max_attempts = self.config.retry_attempts + 1,
                "Pushing detector message with timeout and retry"
            );

            // Attempt send with timeout
            let send_result = timeout(self.config.send_timeout, self.sender.send(msg.clone())).await;
            
            let operation_time = attempt_start.elapsed();
            let operation_time_ms = operation_time.as_micros() as f64 / 1000.0;
            
            // Detect blocking based on operation time and queue depth
            let was_blocked = operation_time_ms > 1.0 || queue_utilization >= self.config.queue_depth_warning_threshold;
            
            match send_result {
                Ok(Ok(())) => {
                    // Success - record metrics and return
                    if was_blocked {
                        info!(
                            operation_time_ms = operation_time_ms,
                            queue_depth = queue_depth_before,
                            channel_capacity = channel_capacity,
                            attempt = attempt + 1,
                            "Successfully sent message after backpressure"
                        );
                        self.record_congestion_event("backpressure_resolved".to_string());
                    }

                    self.circuit_breaker.record_success();
                    self.update_congestion_metrics(queue_depth_before, operation_time_ms, false);

                    return Ok(QueueOperationMetrics::new(
                        start_time.elapsed().as_micros() as f64 / 1000.0,
                        queue_depth_before,
                        was_blocked,
                        channel_capacity,
                    ));
                }
                Ok(Err(e)) => {
                    // Channel closed error - permanent failure
                    error!(
                        error = %e, 
                        operation_time_ms = operation_time_ms,
                        attempt = attempt + 1,
                        "Channel closed, cannot send detector message"
                    );
                    
                    let circuit_opened = self.circuit_breaker.record_failure();
                    if circuit_opened {
                        self.record_congestion_event("circuit_breaker_opened".to_string());
                    }
                    
                    self.update_congestion_metrics(queue_depth_before, operation_time_ms, true);
                    return Err(anyhow::anyhow!("Channel closed: {}", e));
                }
                Err(_timeout_error) => {
                    // Timeout error - potentially transient
                    warn!(
                        operation_time_ms = operation_time_ms,
                        timeout_ms = self.config.send_timeout.as_millis(),
                        queue_depth = queue_depth_before,
                        attempt = attempt + 1,
                        max_attempts = self.config.retry_attempts + 1,
                        "Send operation timed out"
                    );

                    last_error = Some(anyhow::anyhow!(
                        "Send timeout after {}ms (attempt {}/{})", 
                        self.config.send_timeout.as_millis(),
                        attempt + 1,
                        self.config.retry_attempts + 1
                    ));

                    self.record_timeout_error();
                    self.update_congestion_metrics(queue_depth_before, operation_time_ms, true);

                    // If this is not the last attempt, wait for backoff delay
                    if attempt < self.config.retry_attempts {
                        let backoff_delay = self.calculate_backoff_delay(attempt);
                        debug!(
                            backoff_delay_ms = backoff_delay.as_millis(),
                            attempt = attempt + 1,
                            "Waiting before retry due to timeout"
                        );
                        tokio::time::sleep(backoff_delay).await;
                    }
                }
            }
        }

        // All retries exhausted
        let circuit_opened = self.circuit_breaker.record_failure();
        if circuit_opened {
            self.record_congestion_event("circuit_breaker_opened_after_retries".to_string());
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All retry attempts exhausted")))
    }

    /// Calculate exponential backoff delay for retry attempts
    fn calculate_backoff_delay(&self, attempt: u32) -> Duration {
        let base_delay_ms = self.config.retry_base_delay.as_millis() as u64;
        let max_delay_ms = self.config.retry_max_delay.as_millis() as u64;
        
        // Exponential backoff: base * 2^attempt
        let exponential_delay_ms = base_delay_ms * (2_u64.pow(attempt));
        let capped_delay_ms = exponential_delay_ms.min(max_delay_ms);
        
        Duration::from_millis(capped_delay_ms)
    }

    /// Record congestion event for metrics and monitoring
    fn record_congestion_event(&self, event_type: String) {
        if let Ok(mut metrics) = self.congestion_metrics.write() {
            metrics.channel_congestion_events += 1;
            metrics.last_congestion_event = Some(SystemTime::now());
            
            debug!(
                event_type = event_type,
                total_congestion_events = metrics.channel_congestion_events,
                "Recorded congestion event"
            );
        }
    }

    /// Record timeout error for tracking
    fn record_timeout_error(&self) {
        if let Ok(mut metrics) = self.congestion_metrics.write() {
            metrics.send_timeout_errors += 1;
        }
    }

    /// Update congestion metrics with operation details
    fn update_congestion_metrics(&self, queue_depth: usize, operation_time_ms: f64, was_error: bool) {
        if let Ok(mut metrics) = self.congestion_metrics.write() {
            metrics.queue_depth_current = queue_depth;
            
            if was_error {
                metrics.consecutive_failures += 1;
                metrics.backpressure_duration_total_ms += operation_time_ms;
            } else {
                metrics.consecutive_failures = 0;
            }
        }
    }

    /// Get current congestion metrics for monitoring
    pub fn get_congestion_metrics(&self) -> CongestionMetrics {
        self.congestion_metrics.read().unwrap().clone()
    }

    /// Get current circuit breaker state
    pub fn get_circuit_breaker_state(&self) -> CircuitBreakerState {
        self.circuit_breaker.get_state()
    }

    /// Check if the channel is currently experiencing backpressure
    pub fn is_experiencing_backpressure(&self) -> bool {
        let metrics = self.congestion_metrics.read().unwrap();
        let channel_capacity = self.sender.capacity();
        let queue_utilization = metrics.queue_depth_current as f64 / channel_capacity as f64;
        
        queue_utilization >= self.config.queue_depth_warning_threshold || 
        self.circuit_breaker.get_state() != CircuitBreakerState::Closed ||
        metrics.consecutive_failures > 0
    }
}
