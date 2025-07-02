use common::types::{Asset, GraphView, Quantity, TradingPair};
use detector::graph::{Edge, PoolModel, PriceGraph};
use detector::strategies::triangular::TriangularArbitrage;
use detector::strategies::ArbitrageStrategy;
use detector::strategies::TriangularConfig;
use rust_decimal_macros::dec;
use std::str::FromStr;
use std::time::Instant;

/// Integration: build a CLMM price graph with both directions for each pair,
/// and verify the triangular arbitrage strategy detects a profitable cycle.
#[tokio::test]
async fn integration_triangular_detection() {
    // Prepare tick-based pools with exact prices (no slippage):
    // A->B=2×, B->A=0.5× ; B->C=3×, C->B=0.333× ; C->A=4×, A->C=0.25×.
    let a = Asset::from_str("A").unwrap();
    let b = Asset::from_str("B").unwrap();
    let c = Asset::from_str("C").unwrap();

    let mut graph = PriceGraph::new();
    let now = Instant::now();

    let edge1 = Edge {
        pair: TradingPair::new(a.clone(), b.clone()),
        exchange: "dex".to_string(),
        pool_address: "p_ab".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(1000)),
            reserve_y: Quantity(dec!(2000)),
            fee_bps: 0,
        },
        last_updated: now,
    };
    let edge2 = Edge {
        pair: TradingPair::new(b.clone(), a.clone()),
        exchange: "dex".to_string(),
        pool_address: "p_ba".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(2000)),
            reserve_y: Quantity(dec!(1000)),
            fee_bps: 0,
        },
        last_updated: now,
    };
    let edge3 = Edge {
        pair: TradingPair::new(b.clone(), c.clone()),
        exchange: "dex".to_string(),
        pool_address: "p_bc".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(1000)),
            reserve_y: Quantity(dec!(3000)),
            fee_bps: 0,
        },
        last_updated: now,
    };
    let edge4 = Edge {
        pair: TradingPair::new(c.clone(), b.clone()),
        exchange: "dex".to_string(),
        pool_address: "p_cb".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(3000)),
            reserve_y: Quantity(dec!(1000)),
            fee_bps: 0,
        },
        last_updated: now,
    };
    let edge5 = Edge {
        pair: TradingPair::new(c.clone(), a.clone()),
        exchange: "dex".to_string(),
        pool_address: "p_ca".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(1000)),
            reserve_y: Quantity(dec!(500)), // Creates arbitrage opportunity
            fee_bps: 0,
        },
        last_updated: now,
    };
    let edge6 = Edge {
        pair: TradingPair::new(a.clone(), c.clone()),
        exchange: "dex".to_string(),
        pool_address: "p_ac".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(500)),
            reserve_y: Quantity(dec!(1000)),
            fee_bps: 0,
        },
        last_updated: now,
    };

    graph.update_edge(edge1);
    graph.update_edge(edge2);
    graph.update_edge(edge3);
    graph.update_edge(edge4);
    graph.update_edge(edge5);
    graph.update_edge(edge6);

    let view = graph.create_view(&GraphView::All);
    let config = TriangularConfig {
        max_path_length: 3,
        target_dex: None,
    };
    let strat = TriangularArbitrage::new(config);
    let opps = strat.detect_opportunities(&view, 0).await.unwrap();

    assert!(
        !opps.is_empty(),
        "expected at least one triangular arbitrage opportunity"
    );
}
