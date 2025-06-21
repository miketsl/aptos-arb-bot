use std::path::PathBuf;
use bytes::Buf;

use clap::Parser;
use prost::Message;
use serde_json::to_string_pretty;

use market_data_ingestor::data_source::RecordedBatch;

/// proto-to-json: convert recorded protobuf batches to human-readable JSON
#[derive(Parser)]
pub struct Args {
    /// Path to the protobuf file (length-delimited RecordedBatch messages)
    #[clap(long)]
    input: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let data = std::fs::read(&args.input)?;
    let mut buf = bytes::BytesMut::from(&data[..]);
    while buf.has_remaining() {
        let batch = RecordedBatch::decode_length_delimited(&mut buf)?;
        println!("{}", to_string_pretty(&batch)?);
    }
    Ok(())
}