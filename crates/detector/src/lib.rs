//! Price graph, path search, and risk filters.

pub mod api;
pub mod bellman_ford;
pub mod exchange_const;
pub mod gas;
pub mod graph;
pub mod prelude;
pub mod service;
pub mod sizing;
pub mod traits;

use crate::bellman_ford::DetectorConfig;
use crate::service::{DetectorService, DexAdapters, PriceStream};
use crate::traits::{IsExecutor, IsRiskManager};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

// Re-export important types and traits
pub use crate::api::{DetectorApi, DetectorStatus};
pub use crate::bellman_ford::NaiveDetector;
pub use crate::graph::{Edge, PoolModel, PriceGraph, PriceGraphImpl, PriceGraphSnapshot, Tick};
pub use crate::traits::ArbitrageOpportunity;

/// The public interface for the arbitrage detector.
pub struct Detector {
    /// The internal service that manages the detection process.
    service: DetectorService,
}

impl Detector {
    /// Creates a new `Detector`.
    ///
    /// # Arguments
    /// * `config` - The configuration for the detector.
    /// * `price_stream` - A stream of price data.
    /// * `dex_adapters` - A collection of DEX adapters.
    /// * `risk_manager` - The risk manager.
    /// * `executor` - The executor.
    /// * `shutdown_rx` - A receiver for shutdown signals.
    pub fn new(
        config: DetectorConfig,
        price_stream: PriceStream,
        dex_adapters: DexAdapters,
        risk_manager: Arc<dyn IsRiskManager>,
        executor: Arc<dyn IsExecutor>,
        shutdown_rx: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            service: DetectorService::new(
                config,
                price_stream,
                dex_adapters,
                risk_manager,
                executor,
                shutdown_rx,
            ),
        }
    }

    /// Spawns the detector in a background task and returns a join handle.
    pub fn spawn(self) -> tokio::task::JoinHandle<Result<()>> {
        tokio::spawn(async move { self.run().await })
    }

    /// Starts the detector's main loop.
    pub async fn run(self) -> Result<()> {
        self.service.run().await
    }
}

/// Initialize the detector module.
pub fn init() {
    // Placeholder for any initialization required by the detector module
}
