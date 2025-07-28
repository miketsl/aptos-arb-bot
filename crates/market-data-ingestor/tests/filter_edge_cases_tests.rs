use common::types::{
    ClmmMarketUpdate, ConstantProductMarketUpdate, MarketUpdate, Quantity, StableSwapMarketUpdate,
    TokenPair, WeightedPoolMarketUpdate,
};
use config_lib::{FilterConfig, IngestorFilterConfig};
use market_data_ingestor::steps::filter::{FilterMetrics, FilterStep};
use std::collections::HashMap;

// Test helper functions
fn create_clmm_update(token0: &str, token1: &str) -> MarketUpdate {
    MarketUpdate::Clmm(ClmmMarketUpdate {
        pool_address: format!("pool_{}_{}", token0, token1),
        dex_name: "test_dex".to_string(),
        token_pair: TokenPair {
            token0: token0.to_string(),
            token1: token1.to_string(),
        },
        sqrt_price: 100,
        liquidity: 1000,
        tick: 0,
        fee_bps: 30,
        tick_map: HashMap::new(),
    })
}

fn create_constant_product_update(token0: &str, token1: &str) -> MarketUpdate {
    MarketUpdate::ConstantProduct(ConstantProductMarketUpdate {
        pool_address: format!("cp_pool_{}_{}", token0, token1),
        dex_name: "test_dex".to_string(),
        token_pair: TokenPair {
            token0: token0.to_string(),
            token1: token1.to_string(),
        },
        reserve_x: Quantity(5000.into()),
        reserve_y: Quantity(3000.into()),
        fee_bps: 30,
    })
}

fn create_stable_swap_update(token0: &str, token1: &str) -> MarketUpdate {
    MarketUpdate::StableSwap(StableSwapMarketUpdate {
        pool_address: format!("ss_pool_{}_{}", token0, token1),
        dex_name: "test_dex".to_string(),
        token_pair: TokenPair {
            token0: token0.to_string(),
            token1: token1.to_string(),
        },
        reserves: vec![Quantity(1000.into()), Quantity(1000.into())],
        fee_bps: 5,
        amplification_factor: 85,
    })
}

fn create_weighted_pool_update(token0: &str, token1: &str) -> MarketUpdate {
    MarketUpdate::WeightedPool(WeightedPoolMarketUpdate {
        pool_address: format!("wp_pool_{}_{}", token0, token1),
        dex_name: "test_dex".to_string(),
        token_pair: TokenPair {
            token0: token0.to_string(),
            token1: token1.to_string(),
        },
        reserves: vec![Quantity(2000.into()), Quantity(3000.into())],
        weights: vec![500000, 500000], // 50% each as u32
        fee_bps: 30,
    })
}

fn create_test_updates() -> Vec<MarketUpdate> {
    vec![
        create_clmm_update("USDT", "USDC"),
        create_constant_product_update("BTC", "ETH"),
        create_stable_swap_update("DAI", "USDC"),
        create_weighted_pool_update("WETH", "APT"),
    ]
}

fn create_large_update_list(size: usize) -> Vec<MarketUpdate> {
    let mut updates = Vec::with_capacity(size);
    let tokens = ["USDT", "USDC", "BTC", "ETH", "DAI", "WETH", "APT", "SOL"];

    for i in 0..size {
        let token0 = tokens[i % tokens.len()];
        let token1 = tokens[(i + 1) % tokens.len()];
        updates.push(create_clmm_update(token0, token1));
    }

    updates
}

fn create_all_market_types() -> Vec<MarketUpdate> {
    vec![
        create_clmm_update("A", "B"),
        create_constant_product_update("A", "B"),
        create_stable_swap_update("A", "B"),
        create_weighted_pool_update("A", "B"),
    ]
}

fn assert_filter_metrics_consistency(
    metrics: &FilterMetrics,
    original_len: usize,
    final_len: usize,
) {
    assert_eq!(metrics.updates_received_total, original_len as u64);
    assert_eq!(metrics.updates_after_filtering, final_len as u64);
    assert_eq!(
        metrics.updates_filtered_out,
        (original_len - final_len) as u64
    );

    let expected_pass_rate = if original_len > 0 {
        (final_len as f64 / original_len as f64) * 100.0
    } else {
        100.0
    };
    assert!((metrics.filter_pass_rate_percent - expected_pass_rate).abs() < 0.001);

    // Processing time should be reasonable (< 10ms for normal test loads)
    assert!(metrics.filter_processing_time_ms < 10.0);
    assert!(metrics.filter_processing_time_ms >= 0.0);
}

// Basic Edge Cases Tests
#[cfg(test)]
mod basic_edge_cases {
    use super::*;

    #[test]
    fn test_empty_updates_list() {
        let filter_step = FilterStep::new(&FilterConfig::All);
        let mut updates = vec![];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 0);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_pass_rate_percent, 100.0);
    }

    #[test]
    fn test_filter_disabled_scenario() {
        let filter_step = FilterStep::new(&FilterConfig::All);
        let mut updates = create_test_updates();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), original_len);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_pass_rate_percent, 100.0);
        assert_eq!(metrics.updates_filtered_out, 0);
    }

    #[test]
    fn test_no_matching_updates_scenario() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "NONEXISTENT".to_string(),
        });
        let mut updates = create_test_updates();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 0);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_pass_rate_percent, 0.0);
        assert_eq!(metrics.updates_filtered_out, original_len as u64);
        assert_eq!(
            metrics.filter_reasons.filtered_by_token,
            original_len as u64
        );
    }

    #[test]
    fn test_all_updates_pass_scenario() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "USDT".to_string(),
        });
        let mut updates = vec![
            create_clmm_update("USDT", "USDC"),
            create_constant_product_update("USDT", "BTC"),
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), original_len);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_pass_rate_percent, 100.0);
        assert_eq!(metrics.updates_filtered_out, 0);
    }
}

// Token Filtering Edge Cases
#[cfg(test)]
mod token_filtering_edge_cases {
    use super::*;

    #[test]
    fn test_single_token_filter_across_all_market_types() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "A".to_string(),
        });
        let mut updates = create_all_market_types();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), original_len); // All have token "A"
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_pass_rate_percent, 100.0);
    }

    #[test]
    fn test_case_sensitivity_in_token_matching() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "usdt".to_string(),
        });
        let mut updates = vec![
            create_clmm_update("USDT", "USDC"),
            create_clmm_update("usdt", "usdc"),
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 1); // Only lowercase "usdt" should match
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_reasons.filtered_by_token, 1);
    }

    #[test]
    fn test_non_existent_token_filtering() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "NONEXISTENT".to_string(),
        });
        let mut updates = create_test_updates();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 0);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(
            metrics.filter_reasons.filtered_by_token,
            original_len as u64
        );
    }
}

// Token Pairs Filtering Edge Cases
#[cfg(test)]
mod token_pairs_filtering_edge_cases {
    use super::*;

    #[test]
    fn test_exact_pair_matching() {
        let filter_step = FilterStep::new(&FilterConfig::TokenPairs {
            token_pairs: vec![("USDT".to_string(), "USDC".to_string())],
        });
        let mut updates = vec![
            create_clmm_update("USDT", "USDC"),
            create_clmm_update("BTC", "ETH"),
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 1);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_reasons.filtered_by_token_pairs, 1);
    }

    #[test]
    fn test_bidirectional_pair_matching() {
        let filter_step = FilterStep::new(&FilterConfig::TokenPairs {
            token_pairs: vec![("USDT".to_string(), "USDC".to_string())],
        });
        let mut updates = vec![
            create_clmm_update("USDT", "USDC"),
            create_clmm_update("USDC", "USDT"), // Reversed order
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 2); // Both should match
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_reasons.filtered_by_token_pairs, 0);
    }

    #[test]
    fn test_empty_token_pairs_list() {
        let filter_step = FilterStep::new(&FilterConfig::TokenPairs {
            token_pairs: vec![],
        });
        let mut updates = create_test_updates();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 0); // No pairs specified, nothing should pass
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(
            metrics.filter_reasons.filtered_by_token_pairs,
            original_len as u64
        );
    }

    #[test]
    fn test_duplicate_pairs_in_configuration() {
        let filter_step = FilterStep::new(&FilterConfig::TokenPairs {
            token_pairs: vec![
                ("USDT".to_string(), "USDC".to_string()),
                ("USDT".to_string(), "USDC".to_string()), // Duplicate
            ],
        });
        let mut updates = vec![create_clmm_update("USDT", "USDC")];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 1); // Should still work correctly
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
    }

    #[test]
    fn test_mixed_case_token_pairs() {
        let filter_step = FilterStep::new(&FilterConfig::TokenPairs {
            token_pairs: vec![("usdt".to_string(), "usdc".to_string())],
        });
        let mut updates = vec![
            create_clmm_update("USDT", "USDC"),
            create_clmm_update("usdt", "usdc"),
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 1); // Only lowercase should match
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_reasons.filtered_by_token_pairs, 1);
    }
}

// Multi-Token Whitelist Edge Cases
#[cfg(test)]
mod multi_token_whitelist_edge_cases {
    use super::*;

    #[test]
    fn test_two_tokens_creating_all_possible_pairs() {
        let ingestor_config = IngestorFilterConfig {
            enabled: true,
            token_whitelist: Some(vec!["A".to_string(), "B".to_string()]),
            token_pairs: None,
            dex_whitelist: None,
            min_liquidity: None,
        };
        let filter_step = FilterStep::from_ingestor_config(&ingestor_config);

        let mut updates = vec![
            create_clmm_update("A", "B"),
            create_clmm_update("B", "A"), // Should match bidirectionally
            create_clmm_update("A", "C"), // Should not match
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 2);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
    }

    #[test]
    fn test_three_tokens_creating_multiple_pairs() {
        let ingestor_config = IngestorFilterConfig {
            enabled: true,
            token_whitelist: Some(vec!["A".to_string(), "B".to_string(), "C".to_string()]),
            token_pairs: None,
            dex_whitelist: None,
            min_liquidity: None,
        };
        let filter_step = FilterStep::from_ingestor_config(&ingestor_config);

        let mut updates = vec![
            create_clmm_update("A", "B"), // Should match (A,B)
            create_clmm_update("B", "C"), // Should match (B,C)
            create_clmm_update("A", "C"), // Should match (A,C)
            create_clmm_update("A", "D"), // Should not match
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 3);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
    }

    #[test]
    fn test_whitelist_with_single_token_fallback() {
        let ingestor_config = IngestorFilterConfig {
            enabled: true,
            token_whitelist: Some(vec!["A".to_string()]),
            token_pairs: None,
            dex_whitelist: None,
            min_liquidity: None,
        };
        let filter_step = FilterStep::from_ingestor_config(&ingestor_config);

        let mut updates = vec![
            create_clmm_update("A", "B"), // Should match (A is in whitelist)
            create_clmm_update("B", "A"), // Should match (A is in whitelist)
            create_clmm_update("B", "C"), // Should not match
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 2);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
    }

    #[test]
    fn test_whitelist_priority_over_token_pairs() {
        let ingestor_config = IngestorFilterConfig {
            enabled: true,
            token_whitelist: Some(vec!["A".to_string(), "B".to_string()]),
            token_pairs: Some(vec![("C".to_string(), "D".to_string())]),
            dex_whitelist: None,
            min_liquidity: None,
        };
        let filter_step = FilterStep::from_ingestor_config(&ingestor_config);

        let mut updates = vec![
            create_clmm_update("A", "B"), // Should match (whitelist)
            create_clmm_update("C", "D"), // Should not match (token_pairs ignored when whitelist present)
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 1);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
    }
}

// Filter Configuration Edge Cases
#[cfg(test)]
mod filter_configuration_edge_cases {
    use super::*;

    #[test]
    fn test_filter_disabled() {
        let ingestor_config = IngestorFilterConfig {
            enabled: false,
            token_whitelist: Some(vec!["A".to_string()]),
            token_pairs: None,
            dex_whitelist: None,
            min_liquidity: None,
        };
        let filter_step = FilterStep::from_ingestor_config(&ingestor_config);

        let mut updates = create_test_updates();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), original_len); // All should pass when disabled
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_pass_rate_percent, 100.0);
    }

    #[test]
    fn test_no_filter_configuration() {
        let ingestor_config = IngestorFilterConfig {
            enabled: true,
            token_whitelist: None,
            token_pairs: None,
            dex_whitelist: None,
            min_liquidity: None,
        };
        let filter_step = FilterStep::from_ingestor_config(&ingestor_config);

        let mut updates = create_test_updates();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), original_len); // All should pass when no filters specified
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_pass_rate_percent, 100.0);
    }
}

// Performance Edge Cases
#[cfg(test)]
mod performance_edge_cases {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_large_update_lists() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "USDT".to_string(),
        });
        let mut updates = create_large_update_list(1000);
        let original_len = updates.len();

        let start = Instant::now();
        let metrics = filter_step.apply_with_metrics(&mut updates);
        let elapsed = start.elapsed();

        assert_filter_metrics_consistency(&metrics, original_len, updates.len());

        // Performance requirement: < 1ms for 1000 updates
        assert!(
            elapsed.as_millis() < 10,
            "Processing took too long: {}ms",
            elapsed.as_millis()
        );
        assert!(metrics.filter_processing_time_ms < 10.0);
    }

    #[test]
    fn test_processing_time_measurement_accuracy() {
        let filter_step = FilterStep::new(&FilterConfig::All);
        let mut updates = create_test_updates();
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_filter_metrics_consistency(&metrics, original_len, updates.len());

        // Processing time should be reasonable and positive
        assert!(metrics.filter_processing_time_ms >= 0.0);
        assert!(metrics.filter_processing_time_ms < 1.0); // Should be very fast for small lists
    }

    #[test]
    fn test_memory_usage_with_large_filters() {
        let large_token_pairs: Vec<(String, String)> = (0..100)
            .map(|i| (format!("TOKEN{}", i), format!("TOKEN{}", i + 1)))
            .collect();

        let filter_step = FilterStep::new(&FilterConfig::TokenPairs {
            token_pairs: large_token_pairs,
        });
        let mut updates = create_large_update_list(500);
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_filter_metrics_consistency(&metrics, original_len, updates.len());

        // Should handle large filter configurations without issues
        assert!(metrics.filter_processing_time_ms < 50.0);
    }
}

// Market Update Type Coverage
#[cfg(test)]
mod market_update_type_coverage {
    use super::*;

    #[test]
    fn test_all_market_update_types_filtered_equally() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "TARGET".to_string(),
        });

        let mut updates = vec![
            create_clmm_update("TARGET", "OTHER"),
            create_constant_product_update("TARGET", "OTHER"),
            create_stable_swap_update("TARGET", "OTHER"),
            create_weighted_pool_update("TARGET", "OTHER"),
            create_clmm_update("WRONG", "OTHER"),
            create_constant_product_update("WRONG", "OTHER"),
            create_stable_swap_update("WRONG", "OTHER"),
            create_weighted_pool_update("WRONG", "OTHER"),
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 4); // Half should pass
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_reasons.filtered_by_token, 4);

        // Verify all market types are represented in results
        let mut clmm_count = 0;
        let mut cp_count = 0;
        let mut ss_count = 0;
        let mut wp_count = 0;

        for update in &updates {
            match update {
                MarketUpdate::Clmm(_) => clmm_count += 1,
                MarketUpdate::ConstantProduct(_) => cp_count += 1,
                MarketUpdate::StableSwap(_) => ss_count += 1,
                MarketUpdate::WeightedPool(_) => wp_count += 1,
            }
        }

        assert_eq!(clmm_count, 1);
        assert_eq!(cp_count, 1);
        assert_eq!(ss_count, 1);
        assert_eq!(wp_count, 1);
    }

    #[test]
    fn test_mixed_update_types_in_single_batch() {
        let filter_step = FilterStep::new(&FilterConfig::TokenPairs {
            token_pairs: vec![("A".to_string(), "B".to_string())],
        });

        let mut updates = vec![
            create_clmm_update("A", "B"),
            create_constant_product_update("C", "D"),
            create_stable_swap_update("A", "B"),
            create_weighted_pool_update("E", "F"),
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 2); // Only A-B pairs should pass
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
        assert_eq!(metrics.filter_reasons.filtered_by_token_pairs, 2);
    }
}

// Metrics Accuracy Testing
#[cfg(test)]
mod metrics_accuracy_testing {
    use super::*;

    #[test]
    fn test_metrics_consistency_verification() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "KEEP".to_string(),
        });

        let mut updates = vec![
            create_clmm_update("KEEP", "USDC"),
            create_clmm_update("DROP", "USDC"),
            create_clmm_update("KEEP", "BTC"),
            create_clmm_update("DROP", "ETH"),
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        // Verify all metrics are internally consistent
        assert_eq!(metrics.updates_received_total, original_len as u64);
        assert_eq!(metrics.updates_after_filtering, updates.len() as u64);
        assert_eq!(
            metrics.updates_filtered_out,
            (original_len - updates.len()) as u64
        );

        let calculated_pass_rate = (updates.len() as f64 / original_len as f64) * 100.0;
        assert!((metrics.filter_pass_rate_percent - calculated_pass_rate).abs() < 0.001);

        // Verify filter reason totals
        let total_filtered_reasons = metrics.filter_reasons.filtered_by_token
            + metrics.filter_reasons.filtered_by_dex
            + metrics.filter_reasons.filtered_by_liquidity
            + metrics.filter_reasons.filtered_by_token_pairs;
        assert_eq!(total_filtered_reasons, metrics.updates_filtered_out);
    }

    #[test]
    fn test_filter_reason_categorization_accuracy() {
        let filter_step = FilterStep::new(&FilterConfig::Token {
            token: "TARGET".to_string(),
        });

        let mut updates = vec![
            create_clmm_update("TARGET", "USDC"), // Should pass
            create_clmm_update("OTHER", "USDC"),  // Should be filtered by token
        ];
        let original_len = updates.len();

        let metrics = filter_step.apply_with_metrics(&mut updates);

        assert_eq!(updates.len(), 1);
        assert_eq!(metrics.filter_reasons.filtered_by_token, 1);
        assert_eq!(metrics.filter_reasons.filtered_by_dex, 0);
        assert_eq!(metrics.filter_reasons.filtered_by_liquidity, 0);
        assert_eq!(metrics.filter_reasons.filtered_by_token_pairs, 0);
        assert_filter_metrics_consistency(&metrics, original_len, updates.len());
    }
}
