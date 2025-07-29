use anyhow::Result;
use common::types::{Event, MarketUpdate};
use dex_adapters::DexAdapter;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, error, info, warn};

/// Adapter health status for circuit breaker and isolation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterStatus {
    Healthy,
    Degraded,
    Disabled,
    Recovering,
}

/// Error severity classification for adapter failures
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorSeverity {
    Low,      // Transient errors that may resolve quickly
    Medium,   // Persistent errors requiring attention
    High,     // Critical errors affecting adapter functionality
    Critical, // Permanent errors requiring adapter disable
}

/// Error type classification for pattern detection
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ErrorType {
    Parsing,       // Event parsing failures
    Validation,    // Data validation errors
    Network,       // Network-related failures  
    Configuration, // Configuration or setup errors
    Unknown,       // Unclassified errors
}

/// Error context for enhanced debugging and analysis
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub error_type: ErrorType,
    pub severity: ErrorSeverity,
    pub event_type: Option<String>,
    pub adapter_name: String,
    pub timestamp: SystemTime,
    pub error_message: String,
    pub recovery_suggestion: Option<String>,
    pub is_transient: bool,
}

impl ErrorContext {
    pub fn new(
        error_type: ErrorType,
        severity: ErrorSeverity,
        adapter_name: String,
        error_message: String,
        event_type: Option<String>,
        is_transient: bool,
    ) -> Self {
        Self {
            error_type,
            severity,
            event_type,
            adapter_name,
            timestamp: SystemTime::now(),
            error_message,
            recovery_suggestion: None,
            is_transient,
        }
    }

    pub fn with_recovery_suggestion(mut self, suggestion: String) -> Self {
        self.recovery_suggestion = Some(suggestion);
        self
    }
}

/// Comprehensive adapter health tracking with enterprise-grade error isolation
#[derive(Debug)]
pub struct AdapterHealthTracker {
    adapter_name: String,
    status: std::sync::RwLock<AdapterStatus>,
    
    // Error tracking counters
    total_operations: AtomicU64,
    total_errors: AtomicU64,
    consecutive_errors: AtomicU32,
    error_rate_window_start: AtomicU64,
    errors_in_current_window: AtomicU32,
    
    // Error pattern tracking
    error_patterns: std::sync::RwLock<HashMap<ErrorType, u32>>,
    recent_errors: std::sync::RwLock<Vec<ErrorContext>>,
    
    // Circuit breaker state
    last_failure_time: AtomicU64,
    recovery_start_time: AtomicU64,
    circuit_breaker_config: CircuitBreakerConfig,
    
    // Performance tracking
    average_processing_time_ms: std::sync::RwLock<f64>,
    operation_count_for_avg: AtomicU64,
}

/// Configuration for adapter-specific circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub error_threshold: u32,
    pub error_rate_threshold: f64,
    pub recovery_timeout: Duration,
    pub degraded_threshold: u32,
    pub window_duration: Duration,
    pub max_recent_errors: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            error_threshold: 5,
            error_rate_threshold: 0.5, // 50% error rate
            recovery_timeout: Duration::from_secs(30),
            degraded_threshold: 3,
            window_duration: Duration::from_secs(60),
            max_recent_errors: 100,
        }
    }
}

impl AdapterHealthTracker {
    pub fn new(adapter_name: String, config: CircuitBreakerConfig) -> Self {
        Self {
            adapter_name,
            status: std::sync::RwLock::new(AdapterStatus::Healthy),
            total_operations: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            consecutive_errors: AtomicU32::new(0),
            error_rate_window_start: AtomicU64::new(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            ),
            errors_in_current_window: AtomicU32::new(0),
            error_patterns: std::sync::RwLock::new(HashMap::new()),
            recent_errors: std::sync::RwLock::new(Vec::new()),
            last_failure_time: AtomicU64::new(0),
            recovery_start_time: AtomicU64::new(0),
            circuit_breaker_config: config,
            average_processing_time_ms: std::sync::RwLock::new(0.0),
            operation_count_for_avg: AtomicU64::new(0),
        }
    }

    /// Record successful operation
    pub fn record_success(&self, processing_time_ms: f64) {
        self.total_operations.fetch_add(1, Ordering::Relaxed);
        self.consecutive_errors.store(0, Ordering::Relaxed);
        
        // Update rolling average processing time
        self.update_processing_time_average(processing_time_ms);
        
        // Check if we should transition from recovering/degraded to healthy
        let current_status = self.get_status();
        if current_status == AdapterStatus::Recovering || current_status == AdapterStatus::Degraded {
            let consecutive_successes = self.total_operations.load(Ordering::Relaxed) 
                - self.total_errors.load(Ordering::Relaxed);
            
            if consecutive_successes >= 5 { // Require 5 consecutive successes for recovery
                self.transition_to_healthy();
            }
        }
    }

    /// Record error with context and pattern analysis
    pub fn record_error(&self, error_context: ErrorContext) -> bool {
        self.total_operations.fetch_add(1, Ordering::Relaxed);
        self.total_errors.fetch_add(1, Ordering::Relaxed);
        let consecutive = self.consecutive_errors.fetch_add(1, Ordering::Relaxed) + 1;
        
        // Update error rate tracking
        self.update_error_rate_window();
        self.errors_in_current_window.fetch_add(1, Ordering::Relaxed);
        
        // Track error patterns
        self.track_error_pattern(&error_context);
        
        // Store recent error for analysis
        self.store_recent_error(error_context.clone());
        
        // Update last failure time
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.last_failure_time.store(now, Ordering::Relaxed);
        
        // Determine if circuit breaker should activate
        let should_activate = self.should_activate_circuit_breaker(consecutive, &error_context);
        
        if should_activate {
            self.activate_circuit_breaker(&error_context);
        } else if self.should_degrade_adapter(consecutive, &error_context) {
            self.degrade_adapter(&error_context);
        }
        
        should_activate
    }

    /// Check if adapter should allow operations
    pub fn should_allow_operation(&self) -> bool {
        let status = self.get_status();
        
        match status {
            AdapterStatus::Healthy | AdapterStatus::Degraded => true,
            AdapterStatus::Disabled => {
                // Check if recovery period has elapsed
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let last_failure = self.last_failure_time.load(Ordering::Relaxed);
                
                if now - last_failure >= self.circuit_breaker_config.recovery_timeout.as_secs() {
                    self.transition_to_recovering();
                    true
                } else {
                    false
                }
            }
            AdapterStatus::Recovering => true,
        }
    }

    /// Get current adapter status
    pub fn get_status(&self) -> AdapterStatus {
        self.status.read().unwrap().clone()
    }

    /// Get error rate for current window
    pub fn get_error_rate(&self) -> f64 {
        let operations_in_window = self.get_operations_in_window();
        let errors_in_window = self.errors_in_current_window.load(Ordering::Relaxed);
        
        if operations_in_window > 0 {
            errors_in_window as f64 / operations_in_window as f64
        } else {
            0.0
        }
    }

    /// Get comprehensive health metrics
    pub fn get_health_metrics(&self) -> AdapterHealthMetrics {
        let total_ops = self.total_operations.load(Ordering::Relaxed);
        let total_errors = self.total_errors.load(Ordering::Relaxed);
        let consecutive_errors = self.consecutive_errors.load(Ordering::Relaxed);
        
        let overall_error_rate = if total_ops > 0 {
            total_errors as f64 / total_ops as f64
        } else {
            0.0
        };

        AdapterHealthMetrics {
            adapter_name: self.adapter_name.clone(),
            status: self.get_status(),
            total_operations: total_ops,
            total_errors,
            consecutive_errors,
            current_error_rate: self.get_error_rate(),
            overall_error_rate,
            error_patterns: self.error_patterns.read().unwrap().clone(),
            average_processing_time_ms: *self.average_processing_time_ms.read().unwrap(),
            last_failure_time: if self.last_failure_time.load(Ordering::Relaxed) > 0 {
                Some(SystemTime::UNIX_EPOCH + Duration::from_secs(self.last_failure_time.load(Ordering::Relaxed)))
            } else {
                None
            },
        }
    }

    /// Get recent error contexts for analysis
    pub fn get_recent_errors(&self, limit: usize) -> Vec<ErrorContext> {
        let errors = self.recent_errors.read().unwrap();
        errors.iter().rev().take(limit).cloned().collect()
    }

    // Private helper methods
    
    fn update_processing_time_average(&self, processing_time_ms: f64) {
        let count = self.operation_count_for_avg.fetch_add(1, Ordering::Relaxed) + 1;
        let mut avg = self.average_processing_time_ms.write().unwrap();
        *avg = (*avg * (count - 1) as f64 + processing_time_ms) / count as f64;
    }

    fn update_error_rate_window(&self) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window_start = self.error_rate_window_start.load(Ordering::Relaxed);
        
        if now - window_start >= self.circuit_breaker_config.window_duration.as_secs() {
            // Reset window
            self.error_rate_window_start.store(now, Ordering::Relaxed);
            self.errors_in_current_window.store(0, Ordering::Relaxed);
        }
    }

    fn get_operations_in_window(&self) -> u32 {
        // Estimate operations in current window based on total operations and time
        let total_ops = self.total_operations.load(Ordering::Relaxed);
        let window_start = self.error_rate_window_start.load(Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Simple estimation - could be improved with more sophisticated tracking
        let window_duration = (now - window_start).min(self.circuit_breaker_config.window_duration.as_secs());
        ((total_ops as f64 * window_duration as f64 / self.circuit_breaker_config.window_duration.as_secs() as f64) as u32).max(1)
    }

    fn track_error_pattern(&self, error_context: &ErrorContext) {
        if let Ok(mut patterns) = self.error_patterns.write() {
            *patterns.entry(error_context.error_type.clone()).or_insert(0) += 1;
        }
    }

    fn store_recent_error(&self, error_context: ErrorContext) {
        if let Ok(mut errors) = self.recent_errors.write() {
            errors.push(error_context);
            if errors.len() > self.circuit_breaker_config.max_recent_errors {
                errors.remove(0);
            }
        }
    }

    fn should_activate_circuit_breaker(&self, consecutive_errors: u32, error_context: &ErrorContext) -> bool {
        // Activate for consecutive errors threshold
        if consecutive_errors >= self.circuit_breaker_config.error_threshold {
            return true;
        }
        
        // Activate for high error rate
        if self.get_error_rate() >= self.circuit_breaker_config.error_rate_threshold {
            return true;
        }
        
        // Activate immediately for critical errors
        if error_context.severity == ErrorSeverity::Critical {
            return true;
        }
        
        false
    }

    fn should_degrade_adapter(&self, consecutive_errors: u32, _error_context: &ErrorContext) -> bool {
        consecutive_errors >= self.circuit_breaker_config.degraded_threshold &&
        consecutive_errors < self.circuit_breaker_config.error_threshold
    }

    fn activate_circuit_breaker(&self, error_context: &ErrorContext) {
        if let Ok(mut status) = self.status.write() {
            *status = AdapterStatus::Disabled;
            
            warn!(
                adapter = %self.adapter_name,
                consecutive_errors = self.consecutive_errors.load(Ordering::Relaxed),
                error_rate = self.get_error_rate(),
                error_type = ?error_context.error_type,
                error_severity = ?error_context.severity,
                "Adapter circuit breaker activated due to high error rate"
            );
        }
    }

    fn degrade_adapter(&self, error_context: &ErrorContext) {
        if let Ok(mut status) = self.status.write() {
            if *status == AdapterStatus::Healthy {
                *status = AdapterStatus::Degraded;
                
                warn!(
                    adapter = %self.adapter_name,
                    consecutive_errors = self.consecutive_errors.load(Ordering::Relaxed),
                    error_type = ?error_context.error_type,
                    "Adapter degraded due to error pattern"
                );
            }
        }
    }

    fn transition_to_recovering(&self) {
        if let Ok(mut status) = self.status.write() {
            if *status == AdapterStatus::Disabled {
                *status = AdapterStatus::Recovering;
                
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                self.recovery_start_time.store(now, Ordering::Relaxed);
                
                info!(
                    adapter = %self.adapter_name,
                    "Adapter transitioning to recovery mode"
                );
            }
        }
    }

    fn transition_to_healthy(&self) {
        if let Ok(mut status) = self.status.write() {
            let old_status = status.clone();
            *status = AdapterStatus::Healthy;
            
            if old_status != AdapterStatus::Healthy {
                info!(
                    adapter = %self.adapter_name,
                    previous_status = ?old_status,
                    "Adapter recovered to healthy status"
                );
            }
        }
    }
}

/// Health metrics for adapter monitoring
#[derive(Debug, Clone)]
pub struct AdapterHealthMetrics {
    pub adapter_name: String,
    pub status: AdapterStatus,
    pub total_operations: u64,
    pub total_errors: u64,
    pub consecutive_errors: u32,
    pub current_error_rate: f64,
    pub overall_error_rate: f64,
    pub error_patterns: HashMap<ErrorType, u32>,
    pub average_processing_time_ms: f64,
    pub last_failure_time: Option<SystemTime>,
}

/// Enhanced parser with adapter isolation and health tracking
pub struct Parser {
    adapters: HashMap<String, Arc<dyn DexAdapter>>,
    health_trackers: HashMap<String, Arc<AdapterHealthTracker>>,
    circuit_breaker_config: CircuitBreakerConfig,
}

impl Parser {
    pub fn new(adapters: HashMap<String, Arc<dyn DexAdapter>>) -> Self {
        Self::new_with_circuit_breaker_config(adapters, CircuitBreakerConfig::default())
    }
    
    pub fn new_with_circuit_breaker_config(
        adapters: HashMap<String, Arc<dyn DexAdapter>>,
        circuit_breaker_config: CircuitBreakerConfig,
    ) -> Self {
        let mut health_trackers = HashMap::new();
        
        for adapter_name in adapters.keys() {
            let tracker = Arc::new(AdapterHealthTracker::new(
                adapter_name.clone(),
                circuit_breaker_config.clone(),
            ));
            health_trackers.insert(adapter_name.clone(), tracker);
        }
        
        Self {
            adapters,
            health_trackers,
            circuit_breaker_config,
        }
    }

    /// Process events with enhanced error handling and adapter isolation
    pub fn process_events(&self, events: &[Event]) -> Result<Vec<MarketUpdate>> {
        let mut updates = Vec::with_capacity(events.len());
        let mut adapter_processing_times = HashMap::new();
        
        for event in events {
            if let Some(adapter) = self.adapters.get(&event.type_str) {
                if let Some(health_tracker) = self.health_trackers.get(&event.type_str) {
                    // Check if adapter should be allowed to process
                    if !health_tracker.should_allow_operation() {
                        debug!(
                            adapter = %event.type_str,
                            status = ?health_tracker.get_status(),
                            "Skipping event processing - adapter not available"
                        );
                        continue;
                    }
                    
                    let start_time = Instant::now();
                    
                    // Attempt to parse event with comprehensive error handling
                    match adapter.parse_event(event) {
                        Ok(Some(update)) => {
                            let processing_time = start_time.elapsed().as_micros() as f64 / 1000.0;
                            health_tracker.record_success(processing_time);
                            updates.push(update);
                            
                            // Track processing time for adapter performance monitoring
                            adapter_processing_times.insert(event.type_str.clone(), processing_time);
                        }
                        Ok(None) => {
                            // No update generated but no error - record as success
                            let processing_time = start_time.elapsed().as_micros() as f64 / 1000.0;
                            health_tracker.record_success(processing_time);
                        }
                        Err(e) => {
                            let processing_time = start_time.elapsed().as_micros() as f64 / 1000.0;
                            
                            // Classify error for better handling
                            let error_context = self.classify_error(&e, &event.type_str, Some(event.type_str.clone()));
                            
                            // Record error with context
                            let circuit_breaker_activated = health_tracker.record_error(error_context.clone());
                            
                            if circuit_breaker_activated {
                                error!(
                                    adapter = %event.type_str,
                                    error = %e,
                                    processing_time_ms = processing_time,
                                    "Adapter circuit breaker activated"
                                );
                            } else {
                                warn!(
                                    adapter = %event.type_str,
                                    error = %e,
                                    processing_time_ms = processing_time,
                                    error_type = ?error_context.error_type,
                                    error_severity = ?error_context.severity,
                                    consecutive_errors = health_tracker.consecutive_errors.load(Ordering::Relaxed),
                                    "Adapter error recorded"
                                );
                            }
                        }
                    }
                }
            }
        }
        
        // Log adapter performance summary
        if !adapter_processing_times.is_empty() {
            debug!(
                adapter_performance = ?adapter_processing_times,
                total_updates = updates.len(),
                "Event processing completed"
            );
        }
        
        Ok(updates)
    }

    /// Get health metrics for all adapters
    pub fn get_adapter_health_metrics(&self) -> HashMap<String, AdapterHealthMetrics> {
        self.health_trackers
            .iter()
            .map(|(name, tracker)| (name.clone(), tracker.get_health_metrics()))
            .collect()
    }

    /// Get health tracker for specific adapter
    pub fn get_adapter_health_tracker(&self, adapter_name: &str) -> Option<Arc<AdapterHealthTracker>> {
        self.health_trackers.get(adapter_name).cloned()
    }

    /// Check if any adapters are in degraded or disabled state
    pub fn has_degraded_adapters(&self) -> bool {
        self.health_trackers.values().any(|tracker| {
            let status = tracker.get_status();
            status == AdapterStatus::Degraded || status == AdapterStatus::Disabled
        })
    }

    /// Get count of healthy adapters
    pub fn get_healthy_adapter_count(&self) -> usize {
        self.health_trackers
            .values()
            .filter(|tracker| tracker.get_status() == AdapterStatus::Healthy)
            .count()
    }

    // Private helper methods

    fn classify_error(&self, error: &anyhow::Error, adapter_name: &str, event_type: Option<String>) -> ErrorContext {
        let error_message = error.to_string().to_lowercase();
        
        // Classify error type based on error message patterns
        let (error_type, severity, is_transient) = if error_message.contains("parse") || error_message.contains("deserialize") {
            (ErrorType::Parsing, ErrorSeverity::Medium, false)
        } else if error_message.contains("validate") || error_message.contains("invalid") {
            (ErrorType::Validation, ErrorSeverity::Medium, false)
        } else if error_message.contains("network") || error_message.contains("timeout") || error_message.contains("connection") {
            (ErrorType::Network, ErrorSeverity::Low, true)
        } else if error_message.contains("config") || error_message.contains("setup") {
            (ErrorType::Configuration, ErrorSeverity::High, false)
        } else if error_message.contains("critical") || error_message.contains("fatal") {
            (ErrorType::Unknown, ErrorSeverity::Critical, false)
        } else {
            (ErrorType::Unknown, ErrorSeverity::Medium, true)
        };
        
        let mut context = ErrorContext::new(
            error_type,
            severity,
            adapter_name.to_string(),
            error.to_string(),
            event_type,
            is_transient,
        );
        
        // Add recovery suggestions based on error type
        context = match context.error_type {
            ErrorType::Parsing => context.with_recovery_suggestion(
                "Check event schema compatibility and adapter implementation".to_string()
            ),
            ErrorType::Network => context.with_recovery_suggestion(
                "Check network connectivity and retry with backoff".to_string()
            ),
            ErrorType::Configuration => context.with_recovery_suggestion(
                "Review adapter configuration and module addresses".to_string()
            ),
            _ => context,
        };
        
        context
    }
}
