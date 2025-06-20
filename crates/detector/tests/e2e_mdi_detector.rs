use anyhow::Result;
use common::types::{MarketUpdate, TickInfo, TokenPair};
use detector::{
    bellman_ford::DetectorConfig,
    traits::{ArbitrageOpportunity, IsExecutor, IsRiskManager},
    Detector, PriceGraph,
};
use futures::stream::Stream;
use market_data_ingestor::steps::detector_push::DetectorPushStep;
use std::{collections::HashMap, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// Mock Risk Manager
struct MockRiskManager;
#[async_trait::async_trait]
impl IsRiskManager for MockRiskManager {
    async fn assess_risk(&self, _opportunity: &ArbitrageOpportunity) -> Result<bool> {
        Ok(true) // Always approve for this test
    }
}

// Mock Executor
struct MockExecutor;
#[async_trait::async_trait]
impl IsExecutor for MockExecutor {
    async fn execute_trade(&self, _opportunity: &ArbitrageOpportunity) -> Result<()> {
        Ok(()) // Do nothing
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Set up the communication channel
    let (tx, rx) = mpsc::channel::<MarketUpdate>(10);
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);

    // 2. Instantiate the Detector service
    let price_stream: Pin<Box<dyn Stream<Item = MarketUpdate> + Send + Sync>> =
        Box::pin(ReceiverStream::new(rx));

    let detector = Detector::new(
        DetectorConfig::default(),
        price_stream,
        HashMap::new(), // No DEX adapters needed for this test
        Arc::new(MockRiskManager),
        Arc::new(MockExecutor),
        shutdown_rx,
    );

    // 3. Instantiate the MDI's DetectorPush step
    let detector_push_step = DetectorPushStep::new(tx);

    // 4. Create a sample MarketUpdate
    let mut tick_map = HashMap::new();
    tick_map.insert(
        -20,
        TickInfo {
            liquidity_net: 1000,
            liquidity_gross: 10000,
        },
    );
    tick_map.insert(
        10,
        TickInfo {
            liquidity_net: -500,
            liquidity_gross: 5000,
        },
    );

    let market_update = MarketUpdate {
        pool_address: "0x1234".to_string(),
        dex_name: "Tapp".to_string(),
        token_pair: TokenPair {
            token0: "0x1::aptos_coin::AptosCoin".to_string(),
            token1: "0x1::coin::USDC".to_string(),
        },
        sqrt_price: 123456789,
        liquidity: 100000,
        tick: 123,
        fee_bps: 30,
        tick_map,
    };

    // 5. Run the Detector service in a separate task
    let detector_handle = detector.spawn();

    // 6. Push the update to the detector
    detector_push_step
        .push_updates(vec![market_update.clone()])
        .await?;

    // 7. Give the detector a moment to process
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 8. Create the expected edge for comparison
    let expected_edge = detector::translator::market_update_to_edge(&market_update).unwrap();

    // 9. Shutdown the detector
    shutdown_tx.send(()).await.unwrap();

    // 10. Await the detector to finish and get the graph
    let final_graph = match detector_handle.await {
        Ok(Ok(graph)) => graph,
        Ok(Err(e)) => panic!("Detector run failed: {}", e),
        Err(e) => panic!("Detector task panicked: {}", e),
    };

    // 11. Assert the graph state
    let snapshot = final_graph.snapshot();
    assert_eq!(snapshot.node_count(), 2);
    assert_eq!(snapshot.edge_count(), 2); // Forward and reverse

    let found_edge = snapshot
        .all_edges()
        .any(|(_, _, edge)| edge == &expected_edge);

    assert!(found_edge, "The expected edge was not found in the graph");

    println!("E2E test passed!");

    Ok(())
}
