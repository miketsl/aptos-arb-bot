use common::types::{Asset, GraphView, TradingPair};
use detector::graph::{Edge, PoolModel, PriceGraph, Tick};
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

    let tick_ab = Tick {
        price: dec!(2),
        liquidity_gross: dec!(1000),
    };
    let tick_ba = Tick {
        price: dec!(0.5),
        liquidity_gross: dec!(1000),
    };
    let tick_bc = Tick {
        price: dec!(3),
        liquidity_gross: dec!(1000),
    };
    let tick_cb = Tick {
        price: dec!(0.333),
        liquidity_gross: dec!(1000),
    };
    let tick_ca = Tick {
        price: dec!(4),
        liquidity_gross: dec!(1000),
    };
    let tick_ac = Tick {
        price: dec!(0.25),
        liquidity_gross: dec!(1000),
    };

    let mut graph = PriceGraph::new();
    let now = Instant::now();

    for (pair, ticks) in &[
        (
            TradingPair::new(a.clone(), b.clone()),
            vec![tick_ab.clone()],
        ),
        (
            TradingPair::new(b.clone(), a.clone()),
            vec![tick_ba.clone()],
        ),
        (
            TradingPair::new(b.clone(), c.clone()),
            vec![tick_bc.clone()],
        ),
        (
            TradingPair::new(c.clone(), b.clone()),
            vec![tick_cb.clone()],
        ),
        (
            TradingPair::new(c.clone(), a.clone()),
            vec![tick_ca.clone()],
        ),
        (
            TradingPair::new(a.clone(), c.clone()),
            vec![tick_ac.clone()],
        ),
    ] {
        graph.update_edge(Edge {
            pair: pair.clone(),
            exchange: "dex".to_string(),
            pool_address: format!("p_{}{}", pair.asset_x, pair.asset_y),
            model: PoolModel::ConcentratedLiquidity {
                ticks: ticks.clone(),
                fee_bps: 0,
            },
            last_updated: now,
        });
    }

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
