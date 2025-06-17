//! External API for the detector service.

use crate::bellman_ford::DetectorConfig;
use crate::service::{DetectorService, DexAdapters, PriceStream};
use crate::traits::{IsExecutor, IsRiskManager};
use anyhow::Result;
use async_trait::async_trait;

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// External API trait for the detector service.
/// This can be extended to support gRPC or REST endpoints.
#[async_trait]
pub trait DetectorApi: Send + Sync {
    /// Starts the detector and returns a handle to the running task.
    async fn start(&self) -> Result<JoinHandle<Result<()>>>;

    /// Stops the detector service gracefully.
    async fn stop(&self) -> Result<()>;

    /// Gets the current status of the detector.
    async fn status(&self) -> Result<DetectorStatus>;
}

/// Status information for the detector service.
#[derive(Debug, Clone)]
pub struct DetectorStatus {
    /// Whether the detector is currently running.
    pub is_running: bool,
    /// Number of opportunities detected so far.
    pub opportunities_detected: u64,
    /// Number of trades executed so far.
    pub trades_executed: u64,
    /// Last error, if any.
    pub last_error: Option<String>,
}

/// Implementation of the detector API.
pub struct DetectorApiImpl {
    config: DetectorConfig,
    dex_adapters: DexAdapters,
    risk_manager: Arc<dyn IsRiskManager>,
    executor: Arc<dyn IsExecutor>,
}

impl DetectorApiImpl {
    /// Creates a new detector API implementation.
    pub fn new(
        config: DetectorConfig,
        _price_stream: PriceStream,
        dex_adapters: DexAdapters,
        risk_manager: Arc<dyn IsRiskManager>,
        executor: Arc<dyn IsExecutor>,
    ) -> Self {
        Self {
            config,
            dex_adapters,
            risk_manager,
            executor,
        }
    }
}

#[async_trait]
impl DetectorApi for DetectorApiImpl {
    async fn start(&self) -> Result<JoinHandle<Result<()>>> {
        let (_shutdown_tx, shutdown_rx) = mpsc::channel(1);

        // Create a new service instance for each start
        let service = DetectorService::new(
            self.config.clone(),
            // Note: This is a simplified approach. In practice, you'd want to
            // handle stream ownership more carefully
            Box::pin(futures::stream::empty::<Result<crate::prelude::MarketTick>>()) as PriceStream, // Placeholder stream
            self.dex_adapters.clone(),
            self.risk_manager.clone(),
            self.executor.clone(),
            shutdown_rx,
        );

        let handle = tokio::spawn(async move { service.run().await });

        Ok(handle)
    }

    async fn stop(&self) -> Result<()> {
        // In a real implementation, you'd maintain a reference to the shutdown sender
        // For now, this is a no-op
        Ok(())
    }

    async fn status(&self) -> Result<DetectorStatus> {
        // This is a simplified implementation
        // In practice, you'd maintain state about the detector
        Ok(DetectorStatus {
            is_running: false, // Simplified
            opportunities_detected: 0,
            trades_executed: 0,
            last_error: None,
        })
    }
}

/// Creates a new detector API instance with the given configuration.
pub fn create_detector_api(
    config: DetectorConfig,
    price_stream: PriceStream,
    dex_adapters: DexAdapters,
    risk_manager: Arc<dyn IsRiskManager>,
    executor: Arc<dyn IsExecutor>,
) -> impl DetectorApi {
    DetectorApiImpl::new(config, price_stream, dex_adapters, risk_manager, executor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ArbitrageOpportunity;
    // Test implementations - local to avoid cfg(test) issues
    #[derive(Debug, Clone, Default)]
    pub struct DummyRiskManager;

    #[async_trait]
    impl IsRiskManager for DummyRiskManager {
        async fn assess_risk(&self, _opportunity: &ArbitrageOpportunity) -> Result<bool> {
            Ok(true)
        }
    }

    #[derive(Debug, Clone, Default)]
    pub struct DummyExecutor;

    #[async_trait]
    impl IsExecutor for DummyExecutor {
        async fn execute_trade(&self, _opportunity: &ArbitrageOpportunity) -> Result<()> {
            Ok(())
        }
    }
    use futures::stream;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_detector_api_creation() {
        let config = DetectorConfig::default();
        let price_stream =
            Box::pin(stream::empty::<Result<crate::prelude::MarketTick>>()) as PriceStream;
        let dex_adapters = HashMap::new();
        let risk_manager = Arc::new(DummyRiskManager);
        let executor = Arc::new(DummyExecutor);

        let _api = create_detector_api(config, price_stream, dex_adapters, risk_manager, executor);
    }

    #[tokio::test]
    async fn test_detector_status() {
        let config = DetectorConfig::default();
        let price_stream =
            Box::pin(stream::empty::<Result<crate::prelude::MarketTick>>()) as PriceStream;
        let dex_adapters = HashMap::new();
        let risk_manager = Arc::new(DummyRiskManager);
        let executor = Arc::new(DummyExecutor);

        let api = create_detector_api(config, price_stream, dex_adapters, risk_manager, executor);

        let status = api.status().await.unwrap();
        assert!(!status.is_running);
        assert_eq!(status.opportunities_detected, 0);
        assert_eq!(status.trades_executed, 0);
        assert!(status.last_error.is_none());
    }
}
