//! Integration tests for the detector module.

use anyhow::Result;
use async_trait::async_trait;
use detector::bellman_ford::DetectorConfig;
use detector::traits::{ArbitrageOpportunity, IsExecutor, IsRiskManager};
use detector::Detector;
use futures::stream;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

// Test implementations
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

use common::types::{MarketUpdate, TokenPair};

fn mock_market_update() -> MarketUpdate {
    MarketUpdate {
        pool_address: "0x1".to_string(),
        dex_name: "Tapp".to_string(),
        token_pair: TokenPair {
            token0: "0x1::aptos_coin::AptosCoin".to_string(),
            token1: "0x1::coin::USDC".to_string(),
        },
        sqrt_price: 0,
        liquidity: 0,
        tick: 0,
        fee_bps: 0,
        tick_map: HashMap::new(),
    }
}

#[tokio::test]
async fn test_detector_runs_with_noop_services() {
    // Create a detector with no-op services
    let config = DetectorConfig::default();
    let price_stream = Box::pin(stream::empty::<MarketUpdate>()) as detector::service::PriceStream;
    let dex_adapters = HashMap::new();
    let risk_manager = Arc::new(DummyRiskManager);
    let executor = Arc::new(DummyExecutor);
    let (_shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let detector = Detector::new(
        config,
        price_stream,
        dex_adapters,
        risk_manager,
        executor,
        shutdown_rx,
    );

    // Spawn the detector in the background
    let handle = detector.spawn();

    // Give it a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // The detector should shut down gracefully when the stream ends
    let result = tokio::time::timeout(tokio::time::Duration::from_millis(100), handle).await;

    assert!(result.is_ok(), "Detector should shut down gracefully");
    let detector_result = result.unwrap().unwrap();
    assert!(
        detector_result.is_ok(),
        "Detector should complete without errors"
    );
}

#[tokio::test]
async fn test_detector_with_mock_price_stream() {
    use futures::stream;

    // Create a price stream with some mock data
    let mock_updates = vec![mock_market_update(), mock_market_update()];

    let price_stream = Box::pin(stream::iter(mock_updates)) as detector::service::PriceStream;

    let config = DetectorConfig::default();
    let dex_adapters = HashMap::new();
    let risk_manager = Arc::new(DummyRiskManager);
    let executor = Arc::new(DummyExecutor);
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

    let detector = Detector::new(
        config,
        price_stream,
        dex_adapters,
        risk_manager,
        executor,
        shutdown_rx,
    );

    // Spawn the detector
    let handle = detector.spawn();

    // Let it process the mock ticks
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Send shutdown signal
    let _ = shutdown_tx.send(()).await;

    // Wait for shutdown
    let result = tokio::time::timeout(tokio::time::Duration::from_millis(200), handle).await;

    assert!(result.is_ok(), "Detector should shut down gracefully");
    let detector_result = result.unwrap().unwrap();
    assert!(
        detector_result.is_ok(),
        "Detector should complete without errors"
    );
}

#[test]
fn test_detector_config_creation() {
    let config = DetectorConfig::default();

    // Check that the config has reasonable defaults
    assert!(config.min_profit_pct > rust_decimal::Decimal::ZERO);
    assert!(config.min_net_profit > rust_decimal::Decimal::ZERO);
    assert!(config.sizing_config.size_fraction > 0.0);
    assert!(config.sizing_config.slippage_cap > 0.0);
}
