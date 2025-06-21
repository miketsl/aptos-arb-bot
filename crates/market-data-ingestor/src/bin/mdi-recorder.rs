use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStream;
use config_lib::load_config_from_path;
use clap::Parser;
use prost::Message;
use tokio::runtime::Runtime;

use market_data_ingestor::data_source::RecordedBatch;
use market_data_ingestor::ingestor_config::IndexerProcessorConfig;

/// mdi-recorder: capture raw transaction batches to a protobuf file for replay.
#[derive(Parser)]
pub struct Args {
    /// Path to the YAML config file
    #[clap(long)]
    config_path: PathBuf,
    /// Output file for recorded batches (protobuf, length-delimited)
    #[clap(long)]
    output: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Use a tokio runtime for async operations
    let rt = Runtime::new()?;
    rt.block_on(async move {
        // Initialize TLS crypto for gRPC
        rustls::crypto::ring::default_provider()
            .install_default()
            .unwrap();
        // Load config and extract transaction stream settings
        let cfg = load_config_from_path(args.config_path.to_str().unwrap())
            .await?;
        let indexer_cfg = IndexerProcessorConfig::new(
            cfg.transaction_stream_config.clone(),
            cfg.market_data_config.clone(),
        );

        // Set up gRPC stream
        let mut stream =
            TransactionStream::new(indexer_cfg.transaction_stream_config.clone()).await?;

        // Prepare file writer for length-delimited batches
        let file = File::create(&args.output)?;
        let mut writer = BufWriter::new(file);
        println!("Starting recording to {:?}", args.output);

        loop {
            let batch = stream.get_next_transaction_batch().await?;
            let rec = RecordedBatch {
                start_version: batch.start_version,
                end_version: batch.end_version,
                timestamp_ms: batch
                    .start_txn_timestamp
                    .map_or(0, |ts| ts.seconds * 1000 + (ts.nanos as i64) / 1_000_000),
                transactions: batch.transactions,
            };
            // Serialize into an in-memory buffer, then write to file
            let mut buf = bytes::BytesMut::new();
            rec.encode_length_delimited(&mut buf)?;
            writer.write_all(&buf)?;
            writer.flush()?;
        }
    })
}