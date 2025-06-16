use crate::bellman_ford::{DetectorConfig, NaiveDetector};
use crate::graph::{PriceGraph, PriceGraphImpl};
use crate::prelude::*;
use anyhow::Result;
use common::errors::CommonError;
use dex_adapter_trait::{DexAdapter, Exchange};
use futures::stream::{Stream, StreamExt};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver};
use tokio::sync::Mutex;

/// A stream of price ticks.
pub type PriceStream = Pin<Box<dyn Stream<Item = Result<Tick>> + Send>>;

/// A collection of DEX adapters, keyed by their exchange identifier.
pub type DexAdapters = HashMap<Exchange, Arc<dyn DexAdapter>>;

/// The internal service that manages the arbitrage detection process.
/// This is not part of the public API.
pub(crate) struct DetectorService {
    /// The price graph used for arbitrage detection.
    price_graph: Arc<Mutex<PriceGraphImpl>>,
    /// The arbitrage detector.
    detector: NaiveDetector,
    /// A stream of price data.
    price_stream: PriceStream,
    /// Adapters for interacting with DEXs.
    _dex_adapters: DexAdapters,
    /// Receiver for shutdown signals.
    shutdown_rx: Receiver<()>,
}

impl DetectorService {
    /// Creates a new `DetectorService`.
    pub(crate) fn new(
        config: DetectorConfig,
        price_stream: PriceStream,
        dex_adapters: DexAdapters,
        shutdown_rx: Receiver<()>,
    ) -> Self {
        Self {
            price_graph: Arc::new(Mutex::new(PriceGraphImpl::new())),
            detector: NaiveDetector::new(config),
            price_stream,
            _dex_adapters: dex_adapters,
            shutdown_rx,
        }
    }

    /// Starts the main detection loop.
    pub(crate) async fn run(mut self) -> Result<()> {
        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    log::info!("DetectorService shutting down.");
                    break;
                }
                maybe_tick = self.price_stream.next() => {
                    match maybe_tick {
                        Some(Ok(tick)) => {
                            let mut graph = self.price_graph.lock().await;
                            // TODO: Ingest the tick into the graph. This will likely involve
                            // updating or creating an Edge. For now, we'll just log it.
                            log::debug!("Received tick: {:?}", tick);

                            // TODO: Get oracle prices. For now, use a dummy map.
                            let oracle_prices = HashMap::new();

                            // Run detection
                            match self.detector.detect_arbitrage(&graph.snapshot(), &oracle_prices).await {
                                Ok(opportunities) => {
                                    if !opportunities.is_empty() {
                                        log::info!("Found {} arbitrage opportunities.", opportunities.len());
                                    }
                                }
                                Err(e) => {
                                    log::error!("Error during arbitrage detection: {}", e);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            log::error!("Error receiving tick: {}", e);
                        }
                        None => {
                            // Stream ended
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
