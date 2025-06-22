use chrono::Utc;
use common::types::{DetectorMessage, MarketUpdate, TickInfo, TokenPair};
use detector::service::DetectorService;
use detector::strategies::StrategyConfig;
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;

/// Integration: feed three CLMM MarketUpdates (A->B, B->C, C->A), then expect a triangular opportunity.
#[tokio::test]
async fn integration_triangular_detection() {
    // Prepare three simple CLMM pool updates for a profitable triangle
    let mut ticks = HashMap::new();
    // Use tick price 10 for CLMM quoting
    ticks.insert(
        10,
        TickInfo {
            liquidity_net: 0,
            liquidity_gross: 1000u128,
        },
    );

    let update_ab = MarketUpdate {
        pool_address: "p_ab".to_string(),
        dex_name: "D1".to_string(),
        token_pair: TokenPair {
            token0: "A".to_string(),
            token1: "B".to_string(),
        },
        sqrt_price: 0,
        liquidity: 0,
        tick: 0,
        fee_bps: 0,
        tick_map: ticks.clone(),
    };
    let update_bc = MarketUpdate {
        pool_address: "p_bc".to_string(),
        dex_name: "D1".to_string(),
        token_pair: TokenPair {
            token0: "B".to_string(),
            token1: "C".to_string(),
        },
        ..update_ab.clone()
    };
    let update_ca = MarketUpdate {
        pool_address: "p_ca".to_string(),
        dex_name: "D1".to_string(),
        token_pair: TokenPair {
            token0: "C".to_string(),
            token1: "A".to_string(),
        },
        ..update_ab.clone()
    };

    // Set up detector service with the Triangular strategy
    let (tx, rx) = mpsc::channel(16);
    let (op_tx, mut op_rx) = mpsc::channel(4);
    let config = StrategyConfig::Triangular(detector::strategies::TriangularConfig {
        max_path_length: 3,
        target_dex: None,
    });
    let service = DetectorService::new(rx, op_tx, vec![config]).unwrap();
    let handle = tokio::spawn(async move {
        let _ = service.run().await;
    });

    // Send block messages
    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();
    tx.send(DetectorMessage::MarketUpdate(update_ab))
        .await
        .unwrap();
    tx.send(DetectorMessage::MarketUpdate(update_bc))
        .await
        .unwrap();
    tx.send(DetectorMessage::MarketUpdate(update_ca))
        .await
        .unwrap();
    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Await an opportunity
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        op_rx.try_recv().is_ok(),
        "Expected a triangular arbitrage opportunity"
    );

    // Shutdown
    drop(tx);
    let _ = handle.await;
}
