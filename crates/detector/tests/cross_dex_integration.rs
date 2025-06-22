use common::types::{GraphView, Quantity, TradingPair};
use detector::graph::{Edge, PoolModel, PriceGraph};
use detector::strategies::{cross_dex::CrossDexArbitrage, ArbitrageStrategy, CrossDexConfig};
use rust_decimal_macros::dec;
use std::time::Instant;

/// Integration test for cross-dex arbitrage: A->B on D1 vs B->A on D2.
#[tokio::test]
async fn integration_cross_dex_detection() {
    // Construct a price graph with two DEX pools for A/B
    let mut graph = PriceGraph::new();
    let asset_a = common::types::Asset::from("A");
    let asset_b = common::types::Asset::from("B");

    // Dex1 forward pool: price A->B = 1 (reserve_x=1, reserve_y=1)
    let e1 = Edge {
        pair: TradingPair::new(asset_a.clone(), asset_b.clone()),
        exchange: "D1".to_string(),
        pool_address: "p1".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(1)),
            reserve_y: Quantity(dec!(1)),
            fee_bps: 0,
        },
        last_updated: Instant::now(),
    };
    // D2 reverse pool: price B->A = 2 (reserve_x=1, reserve_y=2)
    let rev2 = Edge {
        pair: TradingPair::new(asset_b.clone(), asset_a.clone()),
        exchange: "D2".to_string(),
        pool_address: "p2_rev".to_string(),
        model: PoolModel::ConstantProduct {
            reserve_x: Quantity(dec!(1)),
            reserve_y: Quantity(dec!(2)),
            fee_bps: 0,
        },
        last_updated: Instant::now(),
    };

    // Seed graph
    graph.update_edge(e1);
    graph.update_edge(rev2);

    // Run cross-dex strategy
    let view = graph.create_view(&GraphView::All);
    let strat = CrossDexArbitrage::new(CrossDexConfig::default());
    let opps = strat.detect_opportunities(&view, 1).await.unwrap();
    assert!(
        !opps.is_empty(),
        "Expected at least one cross-dex arbitrage"
    );
}
