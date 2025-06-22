use prometheus::{register_int_counter, register_int_counter_vec, IntCounter, IntCounterVec, Opts};

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
        let disconnected_components = match register_int_counter!(
            "detector_disconnected_components_total",
            "Number of disconnected graph components detected"
        ) {
            Ok(c) => c,
            Err(e) => match e {
                prometheus::Error::AlreadyReg => prometheus::IntCounter::new(
                    "detector_disconnected_components_total",
                    "Number of disconnected graph components detected",
                )
                .unwrap(),
                _ => panic!(
                    "failed to register detector_disconnected_components metric: {}",
                    e
                ),
            },
        };

        let strategy_failures = match register_int_counter_vec!(
            "detector_strategy_failures_total",
            "Number of strategy failures",
            &["strategy"]
        ) {
            Ok(c) => c,
            Err(e) => match e {
                prometheus::Error::AlreadyReg => prometheus::IntCounterVec::new(
                    Opts::new(
                        "detector_strategy_failures_total",
                        "Number of strategy failures",
                    ),
                    &["strategy"],
                )
                .unwrap(),
                _ => panic!(
                    "failed to register detector_strategy_failures metric: {}",
                    e
                ),
            },
        };

        DetectorMetrics {
            disconnected_components,
            strategy_failures,
        }
    }
}
