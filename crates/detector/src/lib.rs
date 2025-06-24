//! # Arbitrage Detector Crate
//!
//! This crate is responsible for detecting arbitrage opportunities from a stream of
//! market data. It is designed as a service that communicates with other parts of
//! the system via channels.
//!
//! ## Architecture
//!
//! The detector service follows a modern async architecture:
//! - Receives `DetectorMessage` events via tokio broadcast channels
//! - Maintains a unified price graph using petgraph
//! - Runs multiple arbitrage strategies in parallel
//! - Publishes opportunities via tokio mpsc channels
//!
//! ## Usage
//!
//! ```rust,no_run
//! use detector::{DetectorService, strategies::StrategyConfig};
//! use tokio::sync::mpsc;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let (tx, rx) = mpsc::channel(100);
//! let (opportunity_tx, opportunity_rx) = mpsc::channel(100);
//! 
//! let strategies = vec![
//!     StrategyConfig::CrossDex(Default::default()),
//!     StrategyConfig::Triangular(detector::strategies::TriangularConfig {
//!         max_path_length: 3,
//!         target_dex: None,
//!     }),
//! ];
//!
//! let service = DetectorService::new(rx, opportunity_tx, strategies)?;
//! service.run().await?;
//! # Ok(())
//! # }
//! ```

// Core modules
pub mod deduplicator;
pub mod error;
pub mod graph;
pub mod service;
pub mod strategies;
pub mod transform;

// Internal modules
mod metrics;

// Re-exports for easy access
pub use service::DetectorService;
pub use error::DetectorError;
pub use strategies::{ArbitrageStrategy, StrategyConfig};
