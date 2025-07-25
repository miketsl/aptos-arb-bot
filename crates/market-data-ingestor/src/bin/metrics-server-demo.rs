use anyhow::Result;
use market_data_ingestor::{http_server, monitoring::MetricsCollector};
use std::sync::{Arc, RwLock};
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("Testing metrics server for dashboard...");

    let metrics_collector = Arc::new(RwLock::new(
        MetricsCollector::new().expect("Failed to create metrics collector"),
    ));

    println!("Starting metrics server on 127.0.0.1:8080");

    // Start metrics server in background
    let server_handle = tokio::spawn(async move {
        if let Err(e) =
            http_server::start_metrics_server("127.0.0.1:8080".to_string(), metrics_collector).await
        {
            eprintln!("Metrics server error: {}", e);
        }
    });

    println!("Metrics server started! Test with:");
    println!("  curl http://localhost:8080/api/health");
    println!("  curl http://localhost:8080/api/metrics");
    println!("Press Ctrl+C to stop...");

    // Wait for Ctrl+C
    signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
    println!("Shutting down...");
    server_handle.abort();

    Ok(())
}
