use prometheus::{register_int_counter, register_int_counter_vec, IntCounter, IntCounterVec};

/// Prometheus metrics for the detector service.
pub struct DetectorMetrics {
    /// Number of disconnected components detected in the price graph.
    pub disconnected_components: IntCounter,
    /// Number of strategy failures, labeled by strategy name.
    pub strategy_failures: IntCounterVec,
}

impl DetectorMetrics {
    /// Creates and registers all detector metrics.
    pub fn new() -> Self {
        let disconnected_components = register_int_counter!(
            "detector_disconnected_components_total",
            "Number of disconnected graph components detected"
        )
        .expect("failed to register detector_disconnected_components metric");

        let strategy_failures = register_int_counter_vec!(
            "detector_strategy_failures_total",
            "Number of strategy failures",
            &["strategy"]
        )
        .expect("failed to register detector_strategy_failures metric");

        DetectorMetrics {
            disconnected_components,
            strategy_failures,
        }
    }
}
