//! Price graph, path search, and risk filters.

pub mod bellman_ford;
pub mod gas;
pub mod graph;
pub mod prelude;
pub mod service;
pub mod sizing;

use crate::bellman_ford::DetectorConfig;
use crate::service::{DetectorService, DexAdapters, PriceStream};
use anyhow::Result;
use tokio::sync::mpsc;

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
    /// * `shutdown_rx` - A receiver for shutdown signals.
    pub fn new(
        config: DetectorConfig,
        price_stream: PriceStream,
        dex_adapters: DexAdapters,
        shutdown_rx: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            service: DetectorService::new(config, price_stream, dex_adapters, shutdown_rx),
        }
    }

    /// Starts the detector's main loop.
    pub async fn run(self) -> Result<()> {
        self.service.run().await
    }
}
