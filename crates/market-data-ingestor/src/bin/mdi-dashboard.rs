use clap::Parser;

#[derive(Parser)]
#[command(name = "mdi-dashboard")]
#[command(about = "Real-time Market Data Ingestor production monitoring dashboard")]
struct Args {
    #[clap(long, default_value = "127.0.0.1:8081")]
    bind_address: String,

    #[clap(long, default_value = "http://127.0.0.1:8080")]
    mdi_endpoint: String,

    #[clap(long, default_value = "1")]
    refresh_interval_seconds: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt::init();

    println!("Starting MDI Dashboard on {}", args.bind_address);
    println!("Monitoring MDI instance at: {}", args.mdi_endpoint);

    // Start the dashboard server that queries live MDI metrics
    market_data_ingestor::http_server::start_dashboard_server(
        args.bind_address,
        args.mdi_endpoint,
        args.refresh_interval_seconds,
    )
    .await?;

    Ok(())
}
