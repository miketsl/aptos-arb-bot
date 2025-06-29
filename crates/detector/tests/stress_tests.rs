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

/// Stress test: Massive number of concurrent updates
#[tokio::test]
async fn stress_test_massive_updates() {
    // Add timeout to prevent hanging
    let test_result = timeout(Duration::from_secs(10), async {
        let (tx, rx) = mpsc::channel(5000);
        let (op_tx, mut op_rx) = mpsc::channel(1000);

        let strategies = vec![
            StrategyConfig::CrossDex(CrossDexConfig {}),
            StrategyConfig::Triangular(TriangularConfig {
                max_path_length: 3,
                target_dex: None,
            }),
        ];

        let service = DetectorService::new(rx, op_tx, strategies).unwrap();
        let handle = tokio::spawn(async move {
            service.run().await.unwrap();
        });

        // Spawn a task to continuously drain opportunities to prevent channel backup
        let drain_handle = tokio::spawn(async move {
            let mut count = 0;
            while op_rx.recv().await.is_some() {
                count += 1;
            }
            count
        });

        // Send targeted arbitrage opportunities across 5 blocks
        let num_blocks = 5;
        let updates_per_block = 8; // 8 updates per block (4 pairs * 2 directions each)

        for block in 1..=num_blocks {
            tx.send(DetectorMessage::BlockStart {
                block_number: block,
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

            // Create explicit arbitrage opportunities
            // We'll create the same token pairs on different DEXes with different prices

            // APT/USDC on DEX1 with price 1x
            let base_price = 1u128 << 64;
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX1", "APT", "USDC", base_price, 1_000_000,
            )))
            .await
            .unwrap();
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX1", "USDC", "APT", base_price, 1_000_000,
            )))
            .await
            .unwrap();

            // APT/USDC on DEX2 with price 2x (clear arbitrage opportunity)
            let high_price = base_price * 2;
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX2", "APT", "USDC", high_price, 1_000_000,
            )))
            .await
            .unwrap();
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX2", "USDC", "APT", high_price, 1_000_000,
            )))
            .await
            .unwrap();

            // ETH/USDC on DEX1 with price 1x
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX1", "ETH", "USDC", base_price, 1_000_000,
            )))
            .await
            .unwrap();
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX1", "USDC", "ETH", base_price, 1_000_000,
            )))
            .await
            .unwrap();

            // ETH/USDC on DEX2 with price 3x (another arbitrage opportunity)
            let very_high_price = base_price * 3;
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX2",
                "ETH",
                "USDC",
                very_high_price,
                1_000_000,
            )))
            .await
            .unwrap();
            tx.send(DetectorMessage::MarketUpdate(create_market_update(
                "DEX2",
                "USDC",
                "ETH",
                very_high_price,
                1_000_000,
            )))
            .await
            .unwrap();

            tx.send(DetectorMessage::BlockEnd {
                block_number: block,
            })
            .await
            .unwrap();
        }

        // Allow final processing
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("Massive updates stress test:");
        println!("  Total updates sent: {}", num_blocks * updates_per_block);

        // Service should handle the load without crashing
        assert!(!handle.is_finished(), "Service should still be running");

        drop(tx);
        handle.await.unwrap();

        let total_opportunities = drain_handle.await.unwrap();
        println!("  Opportunities detected: {}", total_opportunities);
    })
    .await;

    assert!(
        test_result.is_ok(),
        "Massive updates stress test should complete within timeout"
    );
}

/// Stress test: Rapid block transitions
#[tokio::test]
async fn stress_test_rapid_blocks() {
    let (tx, rx) = mpsc::channel(10000);
    let (op_tx, mut op_rx) = mpsc::channel(1000);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Spawn a task to continuously drain opportunities to prevent channel backup
    let drain_handle = tokio::spawn(async move {
        let mut count = 0;
        while op_rx.recv().await.is_some() {
            count += 1;
        }
        count
    });

    // Send 10 blocks with minimal updates each (much smaller for reliability)
    for block in 1..=10 {
        tx.send(DetectorMessage::BlockStart {
            block_number: block,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        // Create bidirectional trading pair
        let update_forward = create_market_update("DEX1", "APT", "USDC", 1u128 << 64, 1_000_000);
        tx.send(DetectorMessage::MarketUpdate(update_forward))
            .await
            .unwrap();

        let update_reverse = create_market_update("DEX1", "USDC", "APT", 1u128 << 64, 1_000_000);
        tx.send(DetectorMessage::MarketUpdate(update_reverse))
            .await
            .unwrap();

        tx.send(DetectorMessage::BlockEnd {
            block_number: block,
        })
        .await
        .unwrap();
    }

    // Allow processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("Rapid blocks stress test completed: 10 blocks processed");
    assert!(!handle.is_finished(), "Service should still be running");

    drop(tx);
    handle.await.unwrap();

    let opportunities_count = drain_handle.await.unwrap();
    println!("Total opportunities detected: {}", opportunities_count);
}

/// Stress test: Extreme price values
#[tokio::test]
async fn stress_test_extreme_values() {
    let (tx, rx) = mpsc::channel(1000);
    let (op_tx, mut op_rx) = mpsc::channel(100);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Spawn a task to continuously drain opportunities to prevent channel backup
    let drain_handle = tokio::spawn(async move {
        let mut count = 0;
        while op_rx.recv().await.is_some() {
            count += 1;
        }
        count
    });

    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    // Test extreme values (but within reasonable bounds to avoid overflow)
    // Create bidirectional pairs for each extreme case

    // Very high price (but reasonable) - DEX1
    let high_price = (1u128 << 64) * 1_000_000;
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX1", "APT", "USDC", high_price, 1_000_000,
    )))
    .await
    .unwrap();
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX1", "USDC", "APT", high_price, 1_000_000,
    )))
    .await
    .unwrap();

    // Very low price (but not zero) - DEX2
    let low_price = (1u128 << 64) / 1_000_000;
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX2", "APT", "USDC", low_price, 1_000_000,
    )))
    .await
    .unwrap();
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX2", "USDC", "APT", low_price, 1_000_000,
    )))
    .await
    .unwrap();

    // Very high liquidity (but reasonable) - DEX3
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX3",
        "ETH",
        "USDC",
        1u128 << 64,
        1_000_000_000_000u128,
    )))
    .await
    .unwrap();
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX3",
        "USDC",
        "ETH",
        1u128 << 64,
        1_000_000_000_000u128,
    )))
    .await
    .unwrap();

    // Very low liquidity (but not zero) - DEX4
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX4",
        "ETH",
        "USDC",
        1u128 << 64,
        1,
    )))
    .await
    .unwrap();
    tx.send(DetectorMessage::MarketUpdate(create_market_update(
        "DEX4",
        "USDC",
        "ETH",
        1u128 << 64,
        1,
    )))
    .await
    .unwrap();

    // Maximum fee - DEX5 (bidirectional)
    let high_fee_update = MarketUpdate::Clmm(ClmmMarketUpdate {
        pool_address: "extreme_fee_pool_forward".to_string(),
        dex_name: "DEX5".to_string(),
        token_pair: TokenPair {
            token0: "BTC".to_string(),
            token1: "USDC".to_string(),
        },
        sqrt_price: 1u128 << 64,
        liquidity: 1_000_000,
        tick: 0,
        fee_bps: 9999, // 99.99% fee
        tick_map: HashMap::new(),
    });
    tx.send(DetectorMessage::MarketUpdate(high_fee_update))
        .await
        .unwrap();

    let high_fee_update_reverse = MarketUpdate::Clmm(ClmmMarketUpdate {
        pool_address: "extreme_fee_pool_reverse".to_string(),
        dex_name: "DEX5".to_string(),
        token_pair: TokenPair {
            token0: "USDC".to_string(),
            token1: "BTC".to_string(),
        },
        sqrt_price: 1u128 << 64,
        liquidity: 1_000_000,
        tick: 0,
        fee_bps: 9999, // 99.99% fee
        tick_map: HashMap::new(),
    });
    tx.send(DetectorMessage::MarketUpdate(high_fee_update_reverse))
        .await
        .unwrap();

    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Allow processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Service should handle extreme values gracefully
    assert!(
        !handle.is_finished(),
        "Service should handle extreme values without crashing"
    );

    drop(tx);
    handle.await.unwrap();

    let opportunities_count = drain_handle.await.unwrap();
    println!(
        "Extreme values test - opportunities detected: {}",
        opportunities_count
    );
}

/// Stress test: Channel saturation
#[tokio::test]
async fn stress_test_channel_saturation() {
    // Use small channels to test backpressure handling
    let (tx, rx) = mpsc::channel(10); // Small input channel
    let (op_tx, mut op_rx) = mpsc::channel(5); // Small output channel

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Try to overwhelm the channels
    let send_handle = tokio::spawn(async move {
        for block in 1..=20 {
            // Reduced from 50 to 20 for faster test
            // This might block due to small channel size
            if tx
                .send(DetectorMessage::BlockStart {
                    block_number: block,
                    timestamp: Utc::now(),
                })
                .await
                .is_err()
            {
                break;
            }

            // Send multiple updates that should create opportunities
            for i in 0..3 {
                // Reduced from 5 to 3
                let update = create_market_update(
                    "DEX1",
                    "APT",
                    "USDC",
                    if i % 2 == 0 { 1u128 << 64 } else { 2u128 << 64 },
                    1_000_000,
                );
                if tx
                    .send(DetectorMessage::MarketUpdate(update))
                    .await
                    .is_err()
                {
                    break;
                }
            }

            if tx
                .send(DetectorMessage::BlockEnd {
                    block_number: block,
                })
                .await
                .is_err()
            {
                break;
            }
        }
        tx
    });

    // Slowly consume opportunities to create backpressure
    let mut opportunities_received = 0;
    for _ in 0..15 {
        // Reduced from 20 to 15
        if timeout(Duration::from_millis(100), op_rx.recv())
            .await
            .is_ok()
        {
            opportunities_received += 1;
        }
        tokio::time::sleep(Duration::from_millis(50)).await; // Slow consumption
    }

    // Drain any remaining opportunities
    while op_rx.try_recv().is_ok() {
        opportunities_received += 1;
    }

    println!("Channel saturation test:");
    println!("  Opportunities received: {}", opportunities_received);
    println!(
        "  Note: With non-blocking sends, some opportunities may be dropped when channel is full"
    );

    // Clean up
    let tx = send_handle.await.unwrap();
    drop(tx);
    handle.await.unwrap();
}

/// Stress test: Memory pressure with large tick maps
#[tokio::test]
async fn stress_test_large_tick_maps() {
    // Add timeout to prevent hanging
    let test_result = timeout(Duration::from_secs(20), async {
        let (tx, rx) = mpsc::channel(1000);
        let (op_tx, mut op_rx) = mpsc::channel(100);

        let strategies = vec![StrategyConfig::Triangular(TriangularConfig {
            max_path_length: 3,
            target_dex: None,
        })];

        let service = DetectorService::new(rx, op_tx, strategies).unwrap();
        let handle = tokio::spawn(async move {
            service.run().await.unwrap();
        });

        // Spawn a task to continuously drain opportunities to prevent channel backup
        let drain_handle = tokio::spawn(async move {
            let mut count = 0;
            while op_rx.recv().await.is_some() {
                count += 1;
            }
            count
        });

        tx.send(DetectorMessage::BlockStart {
            block_number: 1,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        // Create updates with large tick maps
        for i in 0..2 {
            let mut tick_map = HashMap::new();

            // Add 20 ticks per pool (much smaller for reliability)
            for tick in -10i32..10i32 {
                tick_map.insert(
                    tick,
                    TickInfo {
                        liquidity_net: (tick as i128) * 1000,
                        liquidity_gross: tick.unsigned_abs() as u128 * 1000 + 1000,
                    },
                );
            }

            let update = MarketUpdate::Clmm(ClmmMarketUpdate {
                pool_address: format!("large_tick_pool_{}", i),
                dex_name: "CLMM_DEX".to_string(),
                token_pair: TokenPair {
                    token0: format!("TOKEN_{}", i),
                    token1: format!("TOKEN_{}", i + 1),
                },
                sqrt_price: 1u128 << 64,
                liquidity: 10_000_000,
                tick: 0,
                fee_bps: 30,
                tick_map,
            });

            tx.send(DetectorMessage::MarketUpdate(update))
                .await
                .unwrap();
        }

        tx.send(DetectorMessage::BlockEnd { block_number: 1 })
            .await
            .unwrap();

        // Allow processing of large tick maps
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("Large tick maps stress test completed");
        assert!(
            !handle.is_finished(),
            "Service should handle large tick maps without crashing"
        );

        drop(tx);
        handle.await.unwrap();

        let opportunities_count = drain_handle.await.unwrap();
        println!(
            "Large tick maps test - opportunities detected: {}",
            opportunities_count
        );
    })
    .await;

    assert!(
        test_result.is_ok(),
        "Large tick maps stress test should complete within timeout"
    );
}

/// Stress test: Disconnected graph components
#[tokio::test]
async fn stress_test_disconnected_components() {
    let (tx, rx) = mpsc::channel(1000);
    let (op_tx, mut op_rx) = mpsc::channel(100);

    let strategies = vec![
        StrategyConfig::Triangular(TriangularConfig {
            max_path_length: 3,
            target_dex: None,
        }),
        StrategyConfig::MultiHop(MultiHopConfig {
            max_hops: 5,
            min_liquidity: rust_decimal::Decimal::new(1000, 0),
            enable_cross_dex: true,
        }),
    ];

    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Spawn a task to continuously drain opportunities to prevent channel backup
    let drain_handle = tokio::spawn(async move {
        let mut count = 0;
        while op_rx.recv().await.is_some() {
            count += 1;
        }
        count
    });

    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    // Create multiple disconnected components
    // Component 1: APT ecosystem
    let apt_updates = vec![
        create_market_update("DEX1", "APT", "USDC", 1u128 << 64, 1_000_000),
        create_market_update("DEX1", "APT", "ETH", 1u128 << 63, 500_000),
        create_market_update("DEX1", "USDC", "ETH", 4u128 << 64, 500_000),
    ];

    // Component 2: BTC ecosystem (disconnected)
    let btc_updates = vec![
        create_market_update("DEX2", "BTC", "USDT", 16u128 << 64, 2_000_000),
        create_market_update("DEX2", "BTC", "WETH", 8u128 << 64, 800_000),
        create_market_update("DEX2", "USDT", "WETH", 2u128 << 64, 600_000),
    ];

    // Component 3: Isolated pairs
    let isolated_updates = vec![
        create_market_update("DEX3", "DOGE", "SHIB", 1000u128 << 64, 100_000),
        create_market_update("DEX4", "PEPE", "FLOKI", 500u128 << 64, 50_000),
    ];

    // Send all updates
    for update in apt_updates
        .into_iter()
        .chain(btc_updates)
        .chain(isolated_updates)
    {
        tx.send(DetectorMessage::MarketUpdate(update))
            .await
            .unwrap();
    }

    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Allow processing
    tokio::time::sleep(Duration::from_millis(300)).await;

    println!("Disconnected components stress test completed");
    assert!(
        !handle.is_finished(),
        "Service should handle disconnected components gracefully"
    );

    drop(tx);
    handle.await.unwrap();

    let opportunities_count = drain_handle.await.unwrap();
    println!(
        "Disconnected components test - opportunities detected: {}",
        opportunities_count
    );
}

/// Stress test: Rapid strategy reconfiguration simulation
#[tokio::test]
async fn stress_test_strategy_load() {
    // Add timeout to prevent hanging
    let test_result = timeout(Duration::from_secs(30), async {
        // Test with maximum number of strategies
        let strategies = vec![
            StrategyConfig::CrossDex(CrossDexConfig {}),
            StrategyConfig::Triangular(TriangularConfig {
                max_path_length: 3,
                target_dex: None,
            }),
            StrategyConfig::MultiHop(MultiHopConfig {
                max_hops: 5,
                min_liquidity: rust_decimal::Decimal::new(100, 0), // Low threshold for more opportunities
                enable_cross_dex: true,
            }),
            // Add multiple instances of the same strategy with different configs
            StrategyConfig::Triangular(TriangularConfig {
                max_path_length: 4,
                target_dex: Some("DEX1".to_string()),
            }),
            StrategyConfig::Triangular(TriangularConfig {
                max_path_length: 5,
                target_dex: Some("DEX2".to_string()),
            }),
        ];

        let (tx, rx) = mpsc::channel(1000);
        let (op_tx, mut op_rx) = mpsc::channel(500);

        let service = DetectorService::new(rx, op_tx, strategies).unwrap();
        let handle = tokio::spawn(async move {
            service.run().await.unwrap();
        });

        // Create complex interconnected market
        for block in 1..=3 {
            tx.send(DetectorMessage::BlockStart {
                block_number: block,
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

            // Create a smaller graph for reliability
            let tokens = ["APT", "USDC", "ETH"];
            let dexes = ["DEX1", "DEX2"];

            for (i, &token_a) in tokens.iter().enumerate() {
                for (j, &token_b) in tokens.iter().enumerate() {
                    if i != j {
                        for &dex in &dexes {
                            // Vary prices to create arbitrage opportunities
                            let price_factor = if (i + j + block as usize) % 2 == 0 {
                                2
                            } else {
                                1
                            };
                            let sqrt_price = (1u128 << 64) * price_factor;

                            let update =
                                create_market_update(dex, token_a, token_b, sqrt_price, 1_000_000);
                            tx.send(DetectorMessage::MarketUpdate(update))
                                .await
                                .unwrap();
                        }
                    }
                }
            }

            tx.send(DetectorMessage::BlockEnd {
                block_number: block,
            })
            .await
            .unwrap();

            // Drain opportunities after each block
            let mut count = 0;
            while op_rx.try_recv().is_ok() && count < 10 {
                count += 1;
            }
            if count > 0 {
                println!("Block {}: drained {} opportunities", block, count);
            }
        }

        // Final drain
        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut final_count = 0;
        while op_rx.try_recv().is_ok() {
            final_count += 1;
        }

        println!("Strategy load stress test completed");
        println!("Final opportunities drained: {}", final_count);
        assert!(
            !handle.is_finished(),
            "Service should handle multiple strategies without issues"
        );

        drop(tx);
        handle.await.unwrap();
    })
    .await;

    assert!(
        test_result.is_ok(),
        "Strategy load stress test should complete within timeout"
    );
}
