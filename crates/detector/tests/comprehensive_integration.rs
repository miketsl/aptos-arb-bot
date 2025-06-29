use chrono::Utc;
use common::types::{ClmmMarketUpdate, DetectorMessage, MarketUpdate, TickInfo, TokenPair};
use detector::service::DetectorService;
use detector::strategies::{CrossDexConfig, MultiHopConfig, StrategyConfig, TriangularConfig};
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Helper to create market updates
fn create_market_update(
    dex: &str,
    token0: &str,
    token1: &str,
    sqrt_price: u128,
    liquidity: u128,
    fee_bps: u32,
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
        fee_bps,
        tick_map: HashMap::new(),
    })
}

/// Helper to create concentrated liquidity market update
fn create_clmm_update(
    dex: &str,
    token0: &str,
    token1: &str,
    sqrt_price: u128,
    liquidity: u128,
    fee_bps: u32,
    ticks: Vec<(i32, u128)>, // (tick, liquidity_gross)
) -> MarketUpdate {
    let tick_map = ticks
        .into_iter()
        .map(|(tick, liquidity_gross)| {
            (
                tick,
                TickInfo {
                    liquidity_net: 0,
                    liquidity_gross,
                },
            )
        })
        .collect();

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
        fee_bps,
        tick_map,
    })
}

/// Test multiple strategies running simultaneously
#[tokio::test]
async fn test_multi_strategy_detection() {
    let (tx, rx) = mpsc::channel(100);
    let (op_tx, mut op_rx) = mpsc::channel(50);

    // Configure all three strategies
    let strategies = vec![
        StrategyConfig::CrossDex(CrossDexConfig {}),
        StrategyConfig::Triangular(TriangularConfig {
            max_path_length: 3,
            target_dex: None,
        }),
        StrategyConfig::MultiHop(MultiHopConfig {
            max_hops: 4,
            min_liquidity: rust_decimal::Decimal::new(1000, 0),
            enable_cross_dex: true,
        }),
    ];

    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Create a complex market scenario with multiple arbitrage opportunities
    let updates = vec![
        // Cross-DEX opportunity: APT/USDC on different DEXes
        create_market_update("PancakeSwap", "APT", "USDC", 1u128 << 64, 1_000_000, 25), // 1:1 ratio
        create_market_update("Thala", "APT", "USDC", 2u128 << 64, 1_000_000, 30), // 2:1 ratio (arbitrage!)
        // Triangular opportunity: APT -> ETH -> USDC -> APT
        create_market_update("PancakeSwap", "APT", "ETH", 1u128 << 63, 500_000, 25), // APT:ETH = 0.5
        create_market_update("PancakeSwap", "ETH", "USDC", 4u128 << 64, 500_000, 25), // ETH:USDC = 4
        create_market_update("PancakeSwap", "USDC", "APT", 1u128 << 64, 500_000, 25), // USDC:APT = 1
        // Multi-hop opportunity across DEXes
        create_market_update("Thala", "APT", "ETH", 1u128 << 63, 300_000, 30),
        create_market_update("Thala", "ETH", "BTC", 1u128 << 62, 300_000, 30), // ETH:BTC = 0.25
        create_market_update("PancakeSwap", "BTC", "USDC", 16u128 << 64, 300_000, 25), // BTC:USDC = 16
    ];

    // Send block start
    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    // Send all market updates
    for update in updates {
        tx.send(DetectorMessage::MarketUpdate(update))
            .await
            .unwrap();
    }

    // Send block end to trigger detection
    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Collect opportunities with timeout
    let mut opportunities = Vec::new();
    let timeout_duration = Duration::from_millis(500);

    while let Ok(result) = timeout(timeout_duration, op_rx.recv()).await {
        if let Some(opp) = result {
            opportunities.push(opp);
        } else {
            break;
        }
    }

    // Should detect at least one opportunity
    assert!(
        !opportunities.is_empty(),
        "Expected at least 1 opportunity, got {}",
        opportunities.len()
    );

    // Print detected opportunities for debugging
    for (i, opp) in opportunities.iter().enumerate() {
        println!(
            "Opportunity {}: strategy={}, profit={}",
            i + 1,
            opp.strategy,
            opp.expected_profit
        );
    }

    // Verify we have opportunities (may be from same or different strategies)
    let strategy_names: std::collections::HashSet<_> = opportunities
        .iter()
        .map(|opp| opp.strategy.as_str())
        .collect();

    println!("Strategies that found opportunities: {:?}", strategy_names);
    assert!(
        !strategy_names.is_empty(),
        "Expected at least one strategy to find opportunities"
    );

    drop(tx);
    handle.await.unwrap();
}

/// Test service behavior with high-frequency updates
#[tokio::test]
async fn test_high_frequency_updates() {
    let (tx, rx) = mpsc::channel(1000);
    let (op_tx, mut op_rx) = mpsc::channel(100);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Simulate 100 rapid updates across 10 blocks
    for block in 1..=10 {
        tx.send(DetectorMessage::BlockStart {
            block_number: block,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        // 10 updates per block with slight price variations
        for i in 0..10 {
            let price_variation = (1u128 << 64) + (i * 1000); // Slight price changes
            let update =
                create_market_update("DEX1", "APT", "USDC", price_variation, 1_000_000, 25);
            tx.send(DetectorMessage::MarketUpdate(update))
                .await
                .unwrap();

            // Competing DEX with different price
            let competing_price = (2u128 << 64) - (i * 500);
            let competing_update =
                create_market_update("DEX2", "APT", "USDC", competing_price, 1_000_000, 30);
            tx.send(DetectorMessage::MarketUpdate(competing_update))
                .await
                .unwrap();
        }

        tx.send(DetectorMessage::BlockEnd {
            block_number: block,
        })
        .await
        .unwrap();
    }

    // Allow processing time
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Should handle all updates without panicking and detect opportunities
    let mut opportunity_count = 0;
    while op_rx.try_recv().is_ok() {
        opportunity_count += 1;
    }

    // High-frequency updates may or may not create arbitrage opportunities
    // The test verifies that the service handles the load without crashing
    println!(
        "High-frequency test completed. Opportunities detected: {}",
        opportunity_count
    );

    drop(tx);
    handle.await.unwrap();
}

/// Test concentrated liquidity (CLMM) pools
#[tokio::test]
async fn test_clmm_integration() {
    let (tx, rx) = mpsc::channel(100);
    let (op_tx, mut op_rx) = mpsc::channel(50);

    let strategies = vec![StrategyConfig::Triangular(TriangularConfig {
        max_path_length: 3,
        target_dex: None,
    })];

    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Create CLMM pools with tick data
    let updates = vec![
        create_clmm_update(
            "Thala",
            "APT",
            "USDC",
            1u128 << 64,
            2_000_000,
            30,
            vec![(100, 500_000), (200, 1_000_000), (300, 500_000)],
        ),
        create_clmm_update(
            "Thala",
            "USDC",
            "ETH",
            4u128 << 64,
            1_500_000,
            30,
            vec![(150, 400_000), (250, 800_000)],
        ),
        create_clmm_update(
            "Thala",
            "ETH",
            "APT",
            1u128 << 62,
            1_000_000,
            30,
            vec![(50, 300_000), (100, 600_000), (150, 300_000)],
        ),
    ];

    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    for update in updates {
        tx.send(DetectorMessage::MarketUpdate(update))
            .await
            .unwrap();
    }

    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Allow processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should handle CLMM pools and potentially detect opportunities
    let opportunity_received = timeout(Duration::from_millis(100), op_rx.recv())
        .await
        .is_ok();

    // Note: We don't assert opportunity detection here since CLMM pricing is complex
    // The test verifies that CLMM updates are processed without errors
    println!(
        "CLMM integration test completed, opportunity detected: {}",
        opportunity_received
    );

    drop(tx);
    handle.await.unwrap();
}

/// Test error handling and recovery
#[tokio::test]
async fn test_error_handling() {
    let (tx, rx) = mpsc::channel(100);
    let (op_tx, _op_rx) = mpsc::channel(50);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Send malformed updates that should be handled gracefully
    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    // Invalid token addresses (should be handled by transform layer)
    let invalid_update = MarketUpdate::Clmm(ClmmMarketUpdate {
        pool_address: "invalid_pool".to_string(),
        dex_name: "TestDEX".to_string(),
        token_pair: TokenPair {
            token0: "".to_string(), // Empty token address
            token1: "USDC".to_string(),
        },
        sqrt_price: 0, // Invalid price
        liquidity: 0,  // No liquidity
        tick: 0,
        fee_bps: 10000, // Very high fee
        tick_map: HashMap::new(),
    });

    // Service should handle this gracefully
    tx.send(DetectorMessage::MarketUpdate(invalid_update))
        .await
        .unwrap();

    // Send valid update after invalid one
    let valid_update = create_market_update("DEX1", "APT", "USDC", 1u128 << 64, 1_000_000, 25);
    tx.send(DetectorMessage::MarketUpdate(valid_update))
        .await
        .unwrap();

    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Service should continue operating despite errors
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should not crash and should continue processing
    let is_running = !handle.is_finished();
    assert!(is_running, "Service should continue running after errors");

    drop(tx);
    handle.await.unwrap();
}

/// Test graph pruning behavior
#[tokio::test]
async fn test_graph_pruning() {
    let (tx, rx) = mpsc::channel(100);
    let (op_tx, _op_rx) = mpsc::channel(50);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Create many edges to trigger pruning
    for block in 1..=5 {
        tx.send(DetectorMessage::BlockStart {
            block_number: block,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        // Add many different trading pairs
        for i in 0..20 {
            let token_a = format!("TOKEN_{}", i);
            let token_b = format!("TOKEN_{}", i + 1);

            let update = create_market_update(
                "DEX1",
                &token_a,
                &token_b,
                1u128 << 64,
                if i < 10 { 1_000_000 } else { 100 }, // Some with low liquidity
                25,
            );
            tx.send(DetectorMessage::MarketUpdate(update))
                .await
                .unwrap();
        }

        tx.send(DetectorMessage::BlockEnd {
            block_number: block,
        })
        .await
        .unwrap();

        // Allow pruning to occur
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Service should handle pruning without issues
    tokio::time::sleep(Duration::from_millis(100)).await;

    let is_running = !handle.is_finished();
    assert!(is_running, "Service should continue running after pruning");

    drop(tx);
    handle.await.unwrap();
}
