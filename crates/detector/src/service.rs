use crate::bellman_ford::{DetectorConfig, NaiveDetector};
use crate::exchange_const::Exchange;
use crate::graph::{PriceGraph, PriceGraphImpl, PriceGraphSnapshot};
use crate::prelude::*;
use crate::traits::{IsExecutor, IsRiskManager};
use crate::translator;
use anyhow::Result;
use common::types::MarketUpdate;
use dex_adapter_trait::DexAdapter;
use futures::stream::{Stream, StreamExt};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;

/// A stream of market updates.
pub type PriceStream = Pin<Box<dyn Stream<Item = MarketUpdate> + Send + Sync>>;

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
    /// The risk manager.
    risk_manager: Arc<dyn IsRiskManager>,
    /// The executor.
    executor: Arc<dyn IsExecutor>,
    /// Receiver for shutdown signals.
    shutdown_rx: Receiver<()>,
}

impl DetectorService {
    /// Creates a new `DetectorService`.
    pub(crate) fn new(
        config: DetectorConfig,
        price_stream: PriceStream,
        dex_adapters: DexAdapters,
        risk_manager: Arc<dyn IsRiskManager>,
        executor: Arc<dyn IsExecutor>,
        shutdown_rx: Receiver<()>,
    ) -> Self {
        Self {
            price_graph: Arc::new(Mutex::new(PriceGraphImpl::new())),
            detector: NaiveDetector::new(config),
            price_stream,
            _dex_adapters: dex_adapters,
            risk_manager,
            executor,
            shutdown_rx,
        }
    }

    /// Starts the main detection loop.
    pub(crate) async fn run(mut self) -> Result<PriceGraphImpl> {
        loop {
            tokio::select! {
                _ = self.shutdown_rx.recv() => {
                    log::info!("DetectorService shutting down.");
                    break;
                }
                maybe_update = self.price_stream.next() => {
                    match maybe_update {
                        Some(update) => {
                            let mut graph = self.price_graph.lock().await;

                            // Ingest the market update into the graph
                            match translator::market_update_to_edge(&update) {
                                Ok(edge) => {
                                    graph.upsert_pool(edge);
                                    log::debug!("Ingested market update for pool: {}", update.pool_address);
                                }
                                Err(e) => {
                                    log::error!("Failed to translate market update to edge: {}", e);
                                    continue;
                                }
                            }

                            // Get oracle prices - for now, use mock prices based on common assets
                            let oracle_prices = self.get_mock_oracle_prices();

                            // Run detection
                            match self.detector.detect_arbitrage(&graph.snapshot(), &oracle_prices).await {
                                Ok(opportunities) => {
                                    for opportunity in opportunities {
                                        if self.risk_manager.assess_risk(&opportunity).await? {
                                            self.executor.execute_trade(&opportunity).await?;
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::error!("Error during arbitrage detection: {}", e);
                                }
                            }
                        }
                        None => {
                            // Stream ended
                            break;
                        }
                    }
                }
            }
        }
        // Return the final state of the graph
        let graph = Arc::try_unwrap(self.price_graph)
            .map_err(|_| anyhow::anyhow!("Failed to unwrap Arc for price_graph"))?
            .into_inner();
        Ok(graph)
    }

    /// Returns a snapshot of the current price graph.
    pub(crate) async fn get_graph_snapshot(&self) -> PriceGraphSnapshot {
        self.price_graph.lock().await.snapshot()
    }

    /// Ingests a tick into the price graph by creating or updating an edge.
    #[allow(dead_code)]
    async fn ingest_tick_to_graph(
        &self,
        graph: &mut PriceGraphImpl,
        tick: &MarketTick,
    ) -> Result<()> {
        use crate::exchange_const::test_exchange;
        use crate::graph::{Edge, PoolModel};
        use common::types::Quantity;
        use std::time::Instant;

        // Create an edge from the tick data
        let edge = Edge {
            pair: tick.pair.clone(),
            exchange: test_exchange(), // Use a test exchange for now
            model: PoolModel::ConstantProduct {
                reserve_x: Quantity(tick.price * rust_decimal::Decimal::new(10000, 0)), // Mock reserve calculation
                reserve_y: Quantity(rust_decimal::Decimal::new(10000, 0)), // Mock reserve
                fee_bps: 30,                                               // 0.3% fee
            },
            last_updated: Instant::now(),
        };

        // Upsert the edge into the graph
        graph.upsert_pool(edge);
        Ok(())
    }

    /// Gets mock oracle prices for common assets.
    fn get_mock_oracle_prices(&self) -> HashMap<Asset, rust_decimal::Decimal> {
        use rust_decimal_macros::dec;

        let mut prices = HashMap::new();

        // Mock oracle prices in USD
        prices.insert(Asset::from("USDC"), dec!(1.0));
        prices.insert(Asset::from("USDT"), dec!(1.0));
        prices.insert(Asset::from("APT"), dec!(8.0));
        prices.insert(Asset::from("ETH"), dec!(2000.0));
        prices.insert(Asset::from("BTC"), dec!(45000.0));
        prices.insert(Asset::from("MOJO"), dec!(0.5));

        prices
    }
}
