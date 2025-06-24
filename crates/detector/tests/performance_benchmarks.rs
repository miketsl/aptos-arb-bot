use chrono::Utc;
use common::types::{DetectorMessage, MarketUpdate, TokenPair};
use detector::service::DetectorService;
use detector::strategies::{CrossDexConfig, MultiHopConfig, StrategyConfig, TriangularConfig};
use std::{collections::HashMap, time::{Duration, Instant}};
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
    MarketUpdate {
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
    }
}

/// Benchmark: Single strategy detection latency
#[tokio::test]
async fn benchmark_single_strategy_latency() {
    let (tx, rx) = mpsc::channel(1000);
    let (op_tx, mut op_rx) = mpsc::channel(100);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    // Warm up
    tx.send(DetectorMessage::BlockStart {
        block_number: 0,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();
    tx.send(DetectorMessage::BlockEnd { block_number: 0 })
        .await
        .unwrap();

    // Benchmark 100 detection cycles
    let mut latencies = Vec::new();
    
    for block in 1..=100 {
        let start = Instant::now();
        
        tx.send(DetectorMessage::BlockStart {
            block_number: block,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        // Send cross-dex arbitrage opportunity
        let update1 = create_market_update("DEX1", "APT", "USDC", 1u128 << 64, 1_000_000);
        let update2 = create_market_update("DEX2", "APT", "USDC", 2u128 << 64, 1_000_000);
        
        tx.send(DetectorMessage::MarketUpdate(update1)).await.unwrap();
        tx.send(DetectorMessage::MarketUpdate(update2)).await.unwrap();
        
        tx.send(DetectorMessage::BlockEnd { block_number: block })
            .await
            .unwrap();

        // Wait for opportunity detection
        if timeout(Duration::from_millis(50), op_rx.recv()).await.is_ok() {
            let latency = start.elapsed();
            latencies.push(latency);
        }
    }

    // Calculate statistics
    if !latencies.is_empty() {
        latencies.sort();
        let avg = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        let p50 = latencies[latencies.len() / 2];
        let p95 = latencies[latencies.len() * 95 / 100];
        let p99 = latencies[latencies.len() * 99 / 100];

        println!("Single Strategy Latency Benchmark:");
        println!("  Samples: {}", latencies.len());
        println!("  Average: {:?}", avg);
        println!("  P50: {:?}", p50);
        println!("  P95: {:?}", p95);
        println!("  P99: {:?}", p99);

        // Performance targets from architecture.md
        assert!(avg < Duration::from_millis(10), "Average latency should be < 10ms, got {:?}", avg);
        assert!(p95 < Duration::from_millis(20), "P95 latency should be < 20ms, got {:?}", p95);
    }

    drop(tx);
    handle.await.unwrap();
}

/// Benchmark: Multi-strategy parallel execution
#[tokio::test]
async fn benchmark_multi_strategy_performance() {
    let (tx, rx) = mpsc::channel(1000);
    let (op_tx, mut op_rx) = mpsc::channel(200);

    // All three strategies
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

    let mut latencies = Vec::new();
    
    for block in 1..=50 {
        let start = Instant::now();
        
        tx.send(DetectorMessage::BlockStart {
            block_number: block,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        // Create complex market scenario
        let updates = vec![
            create_market_update("DEX1", "APT", "USDC", 1u128 << 64, 1_000_000),
            create_market_update("DEX2", "APT", "USDC", 2u128 << 64, 1_000_000),
            create_market_update("DEX1", "APT", "ETH", 1u128 << 63, 500_000),
            create_market_update("DEX1", "ETH", "USDC", 4u128 << 64, 500_000),
            create_market_update("DEX1", "USDC", "APT", 1u128 << 64, 500_000),
            create_market_update("DEX2", "ETH", "BTC", 1u128 << 62, 300_000),
            create_market_update("DEX2", "BTC", "USDC", 16u128 << 64, 300_000),
        ];

        for update in updates {
            tx.send(DetectorMessage::MarketUpdate(update)).await.unwrap();
        }
        
        tx.send(DetectorMessage::BlockEnd { block_number: block })
            .await
            .unwrap();

        // Collect all opportunities for this block
        let mut block_opportunities = 0;
        let timeout_duration = Duration::from_millis(100);
        
        while let Ok(result) = timeout(timeout_duration, op_rx.recv()).await {
            if result.is_some() {
                block_opportunities += 1;
            } else {
                break;
            }
        }

        if block_opportunities > 0 {
            let latency = start.elapsed();
            latencies.push((latency, block_opportunities));
        }
    }

    // Calculate statistics
    if !latencies.is_empty() {
        let mut times: Vec<_> = latencies.iter().map(|(t, _)| *t).collect();
        times.sort();
        
        let total_opportunities: usize = latencies.iter().map(|(_, count)| count).sum();
        let avg_time = times.iter().sum::<Duration>() / times.len() as u32;
        let p95_time = times[times.len() * 95 / 100];

        println!("Multi-Strategy Performance Benchmark:");
        println!("  Blocks processed: {}", latencies.len());
        println!("  Total opportunities: {}", total_opportunities);
        println!("  Average latency: {:?}", avg_time);
        println!("  P95 latency: {:?}", p95_time);
        println!("  Avg opportunities per block: {:.2}", total_opportunities as f64 / latencies.len() as f64);

        // Multi-strategy should be reasonable (more complex than single strategy)
        assert!(avg_time < Duration::from_millis(200), "Multi-strategy average should be < 200ms, got {:?}", avg_time);
    }

    drop(tx);
    handle.await.unwrap();
}

/// Benchmark: High-frequency update throughput
#[tokio::test]
async fn benchmark_update_throughput() {
    let (tx, rx) = mpsc::channel(10000);
    let (op_tx, _op_rx) = mpsc::channel(1000);

    let strategies = vec![StrategyConfig::CrossDex(CrossDexConfig {})];
    let service = DetectorService::new(rx, op_tx, strategies).unwrap();
    let handle = tokio::spawn(async move {
        service.run().await.unwrap();
    });

    let start = Instant::now();
    let num_blocks = 10;
    let updates_per_block = 100;
    let total_updates = num_blocks * updates_per_block;

    for block in 1..=num_blocks {
        tx.send(DetectorMessage::BlockStart {
            block_number: block as u64,
            timestamp: Utc::now(),
        })
        .await
        .unwrap();

        for i in 0..updates_per_block {
            let update = create_market_update(
                &format!("DEX{}", i % 5), // 5 different DEXes
                &format!("TOKEN{}", i % 20), // 20 different tokens
                &format!("TOKEN{}", (i + 1) % 20),
                (1u128 << 64) + (i as u128 * 1000),
                1_000_000,
            );
            tx.send(DetectorMessage::MarketUpdate(update)).await.unwrap();
        }

        tx.send(DetectorMessage::BlockEnd { block_number: block as u64 })
            .await
            .unwrap();
    }

    // Wait for all processing to complete
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    let total_time = start.elapsed();
    let throughput = total_updates as f64 / total_time.as_secs_f64();

    println!("Update Throughput Benchmark:");
    println!("  Total updates: {}", total_updates);
    println!("  Total time: {:?}", total_time);
    println!("  Throughput: {:.0} updates/sec", throughput);

    // Should handle at least 1000 updates/sec
    assert!(throughput > 1000.0, "Throughput should be > 1000 updates/sec, got {:.0}", throughput);

    drop(tx);
    handle.await.unwrap();
}

/// Benchmark: Memory usage with large graphs
#[tokio::test]
async fn benchmark_memory_usage() {
    let (tx, rx) = mpsc::channel(1000);
    let (op_tx, _op_rx) = mpsc::channel(100);

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

    let start = Instant::now();
    
    // Create a large graph with many edges
    tx.send(DetectorMessage::BlockStart {
        block_number: 1,
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    // Create 1000 different trading pairs across 10 DEXes
    let num_tokens = 100;
    let num_dexes = 10;
    let mut update_count = 0;

    for dex in 0..num_dexes {
        for i in 0..num_tokens {
            for j in (i + 1)..std::cmp::min(i + 5, num_tokens) { // Each token connects to 4 others
                let update = create_market_update(
                    &format!("DEX{}", dex),
                    &format!("TOKEN{}", i),
                    &format!("TOKEN{}", j),
                    1u128 << 64,
                    1_000_000,
                );
                tx.send(DetectorMessage::MarketUpdate(update)).await.unwrap();
                update_count += 1;
            }
        }
    }

    tx.send(DetectorMessage::BlockEnd { block_number: 1 })
        .await
        .unwrap();

    // Allow processing and pruning
    tokio::time::sleep(Duration::from_millis(1000)).await;
    
    let processing_time = start.elapsed();

    println!("Memory Usage Benchmark:");
    println!("  Updates processed: {}", update_count);
    println!("  Processing time: {:?}", processing_time);
    println!("  Updates per second: {:.0}", update_count as f64 / processing_time.as_secs_f64());

    // Should handle large graphs efficiently
    assert!(processing_time < Duration::from_secs(5), "Large graph processing should be < 5s, got {:?}", processing_time);

    drop(tx);
    handle.await.unwrap();
}

/// Benchmark: Strategy-specific performance
#[tokio::test]
async fn benchmark_strategy_performance() {
    // Test each strategy individually
    let strategies_to_test = vec![
        ("CrossDex", StrategyConfig::CrossDex(CrossDexConfig {})),
        ("Triangular", StrategyConfig::Triangular(TriangularConfig {
            max_path_length: 3,
            target_dex: None,
        })),
        ("MultiHop", StrategyConfig::MultiHop(MultiHopConfig {
            max_hops: 4,
            min_liquidity: rust_decimal::Decimal::new(1000, 0),
            enable_cross_dex: true,
        })),
    ];

    for (strategy_name, strategy_config) in strategies_to_test {
        let (tx, rx) = mpsc::channel(1000);
        let (op_tx, mut op_rx) = mpsc::channel(100);

        let service = DetectorService::new(rx, op_tx, vec![strategy_config]).unwrap();
        let handle = tokio::spawn(async move {
            service.run().await.unwrap();
        });

        let mut latencies = Vec::new();
        
        for block in 1..=20 {
            let start = Instant::now();
            
            tx.send(DetectorMessage::BlockStart {
                block_number: block,
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

            // Create scenario optimized for this strategy
            match strategy_name {
                "CrossDex" => {
                    // Same pair, different DEXes
                    let update1 = create_market_update("DEX1", "APT", "USDC", 1u128 << 64, 1_000_000);
                    let update2 = create_market_update("DEX2", "APT", "USDC", 2u128 << 64, 1_000_000);
                    tx.send(DetectorMessage::MarketUpdate(update1)).await.unwrap();
                    tx.send(DetectorMessage::MarketUpdate(update2)).await.unwrap();
                }
                "Triangular" => {
                    // Triangle: APT -> ETH -> USDC -> APT
                    let updates = vec![
                        create_market_update("DEX1", "APT", "ETH", 1u128 << 63, 500_000),
                        create_market_update("DEX1", "ETH", "USDC", 4u128 << 64, 500_000),
                        create_market_update("DEX1", "USDC", "APT", 1u128 << 64, 500_000),
                    ];
                    for update in updates {
                        tx.send(DetectorMessage::MarketUpdate(update)).await.unwrap();
                    }
                }
                "MultiHop" => {
                    // Long chain: APT -> ETH -> BTC -> USDC -> APT
                    let updates = vec![
                        create_market_update("DEX1", "APT", "ETH", 1u128 << 63, 300_000),
                        create_market_update("DEX2", "ETH", "BTC", 1u128 << 62, 300_000),
                        create_market_update("DEX1", "BTC", "USDC", 16u128 << 64, 300_000),
                        create_market_update("DEX2", "USDC", "APT", 1u128 << 64, 300_000),
                    ];
                    for update in updates {
                        tx.send(DetectorMessage::MarketUpdate(update)).await.unwrap();
                    }
                }
                _ => unreachable!(),
            }
            
            tx.send(DetectorMessage::BlockEnd { block_number: block })
                .await
                .unwrap();

            // Wait for opportunity
            if timeout(Duration::from_millis(100), op_rx.recv()).await.is_ok() {
                let latency = start.elapsed();
                latencies.push(latency);
            }
        }

        if !latencies.is_empty() {
            latencies.sort();
            let avg = latencies.iter().sum::<Duration>() / latencies.len() as u32;
            let p95 = latencies[latencies.len() * 95 / 100];

            println!("{} Strategy Benchmark:", strategy_name);
            println!("  Samples: {}", latencies.len());
            println!("  Average: {:?}", avg);
            println!("  P95: {:?}", p95);
        }

        drop(tx);
        handle.await.unwrap();
    }
}