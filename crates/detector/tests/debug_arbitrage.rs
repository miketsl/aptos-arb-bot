use chrono::Utc;
use common::types::{ClmmMarketUpdate, DetectorMessage, MarketUpdate, TokenPair};
use detector::service::DetectorService;
use detector::strategies::{CrossDexConfig, StrategyConfig};
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;

/// Helper to create market updates
fn create_market_update(
    dex: &str,
    token0: &str,
    token1: &str,
    sqrt_price: u128,
    liquidity: u128,
) -> MarketUpdate {
    MarketUpdate::Clmm(ClmmMarketUpdate {
        pool_address: format!("{}-{}-{}", dex, token0, token1),
        dex_name: dex.to_string(),
        token_pair: TokenPair {
            token0: token0.to_string(),
            token1: token1.to_string(),
        },
        sqrt_price,
        liquidity,
        tick: 0,
        fee_bps: 25,
        tick_map: HashMap::new(),
    })
}

#[tokio::test]
async fn debug_simple_arbitrage() {
    let (tx, rx) = mpsc::channel(100);
    let (op_tx, mut op_rx) = mpsc::channel(10);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Spawn a task to drain opportunities
    let drain_handle = tokio::spawn(async move {
        let mut count = 0;
        while let Some(opp) = op_rx.recv().await {
            count += 1;
            println!(
                "Opportunity {}: strategy={}, profit={}, input={}",
                count, opp.strategy, opp.expected_profit, opp.input_amount
            );
        }
        count
    });

    // Send block start
    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    // Create a clear arbitrage opportunity:
    // DEX1: APT/USDC at price 1 (1 APT = 1 USDC)
    // DEX2: APT/USDC at price 2 (1 APT = 2 USDC)
    // This should create a clear arbitrage: buy APT on DEX1, sell on DEX2

    println!("Creating APT/USDC pool on DEX1 with price 1:1");
    let update1 = create_market_update("DEX1", "APT", "USDC", 1u128 << 64, 1_000_000);
    tx.send(DetectorMessage::MarketUpdate(update1))
        .await
        .unwrap();

    println!("Creating USDC/APT pool on DEX1 (reverse direction)");
    let update1_rev = create_market_update("DEX1", "USDC", "APT", 1u128 << 64, 1_000_000);
    tx.send(DetectorMessage::MarketUpdate(update1_rev))
        .await
        .unwrap();

    println!("Creating APT/USDC pool on DEX2 with price 1:2");
    let update2 = create_market_update("DEX2", "APT", "USDC", 2u128 << 64, 1_000_000);
    tx.send(DetectorMessage::MarketUpdate(update2))
        .await
        .unwrap();

    println!("Creating USDC/APT pool on DEX2 (reverse direction)");
    let update2_rev = create_market_update("DEX2", "USDC", "APT", (1u128 << 64) / 2, 1_000_000);
    tx.send(DetectorMessage::MarketUpdate(update2_rev))
        .await
        .unwrap();

    // Send block end to trigger detection
    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Allow processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    drop(tx);
    handle.await.unwrap();

    let opportunities_count = drain_handle.await.unwrap();
    println!("Total opportunities detected: {}", opportunities_count);

    // We should detect at least one opportunity
    assert!(
        opportunities_count > 0,
        "Should detect arbitrage opportunity between DEX1 and DEX2"
    );
}
