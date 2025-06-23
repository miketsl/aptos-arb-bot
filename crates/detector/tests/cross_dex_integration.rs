use chrono::Utc;
use common::types::{DetectorMessage, MarketUpdate, TokenPair};
use detector::service::DetectorService;
use detector::strategies::{CrossDexConfig, StrategyConfig};
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;

/// Integration: feed forward and reverse ConstantProduct updates for two DEXes,
/// and expect the CrossDexArbitrage strategy to detect a profitable cycle.
#[tokio::test]
async fn integration_cross_dex_detection() {
    // Single-tick CLMM pools (empty tick_map) to drive ConstantProduct edges.
    let mk_update = |dex: &str, t0: &str, t1: &str, sqrt_q64: u128| MarketUpdate {
        pool_address: format!("{}-{}-{}", dex, t0, t1),
        dex_name: dex.to_string(),
        token_pair: TokenPair {
            token0: t0.to_string(),
            token1: t1.to_string(),
        },
        sqrt_price: sqrt_q64,
        liquidity: 1_000_000,
        tick: 0,
        fee_bps: 0,
        tick_map: HashMap::new(),
    };

    // Desired price ratios:
    //  D1 A->B = 1.5   (sqrt_price = sqrt(1.5) * 2^64)
    //  D1 B->A = 2/3   (sqrt_price = sqrt(2/3) * 2^64)
    //  D2 A->B = 2.0   (sqrt_price = sqrt(2) * 2^64)
    //  D2 B->A = 0.5   (sqrt_price = sqrt(0.5) * 2^64)
    let sqrt_d1_ab = 22592555198148960256u128;
    let sqrt_d1_ba = 15061703465432641536u128;
    let sqrt_d2_ab = 26087635650665566208u128;
    let sqrt_d2_ba = 13043817825332783104u128;

    let updates = vec![
        mk_update("D1", "A", "B", sqrt_d1_ab),
        mk_update("D1", "B", "A", sqrt_d1_ba),
        mk_update("D2", "A", "B", sqrt_d2_ab),
        mk_update("D2", "B", "A", sqrt_d2_ba),
    ];

    let (tx, rx) = mpsc::channel(32);
    let (op_tx, mut op_rx) = mpsc::channel(4);
    let cfg = StrategyConfig::CrossDex(CrossDexConfig {});
    let service = DetectorService::new(rx, op_tx, vec![cfg]).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();
    for u in updates {
        tx.send(DetectorMessage::MarketUpdate(u)).await.unwrap();
    }
    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Allow detection to run
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        op_rx.try_recv().is_ok(),
        "Expected cross-dex arbitrage opportunity"
    );

    drop(tx);
    handle.await.unwrap();
}
