//! # Arbitrage Detector Crate
//!
//! This crate is responsible for detecting arbitrage opportunities from a stream of
//! market data. It is designed as a service that communicates with other parts of
//! the system via channels.

pub mod deduplicator;
pub mod graph;
pub mod service;
pub mod strategies;
pub mod transform;

pub mod error;
mod metrics;

// Re-export the main service struct for easy access.
pub use service::DetectorService;
