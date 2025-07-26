use market_data_ingestor::{http_server, monitoring::MetricsCollector};
use std::sync::{Arc, RwLock};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("Testing metrics server standalone...");

    let metrics_collector = Arc::new(RwLock::new(
        MetricsCollector::new().expect("Failed to create metrics collector"),
    ));

    println!("Metrics collector created, starting server on 127.0.0.1:8080");

    http_server::start_metrics_server("127.0.0.1:8080".to_string(), metrics_collector).await?;

    Ok(())
}
