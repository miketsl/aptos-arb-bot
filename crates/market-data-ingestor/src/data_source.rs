use async_trait::async_trait;
use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::{
    TransactionStream, TransactionStreamConfig, TransactionsPBResponse,
};

use std::{
    fs,
    time::{Duration, Instant},
};
use prost::Message;
use bytes::Bytes;
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction as ProtoTransaction;
/// Abstracts the source of transaction data, allowing for live or prerecorded streams.
#[async_trait]
pub trait DataSource: Send {
    /// Fetch the next batch of transactions from this source.
    async fn get_next_batch(&mut self) -> Result<TransactionsPBResponse>;
}

/// Live data source using the Aptos gRPC transaction stream.
pub struct GrpcSource {
    inner: TransactionStream,
}

impl GrpcSource {
    /// Wrap an existing `TransactionStream` from the given configuration.
    pub async fn new(config: TransactionStreamConfig) -> Result<Self> {
        let inner = TransactionStream::new(config).await?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl DataSource for GrpcSource {
    async fn get_next_batch(&mut self) -> Result<TransactionsPBResponse> {
        let batch = self.inner.get_next_transaction_batch().await?;
        Ok(batch)
    }
}

/// File-based data source for replaying prerecorded protobuf data (RecordedBatch).
pub struct FileSource {
    buf: Bytes,
    first_timestamp_ms: Option<i64>,
    start_instant: Instant,
    replay_speed: f64,
}

/// Protobuf message for recorded batches, matching the architecture spec.
#[derive(prost::Message)]
pub struct RecordedBatch {
    #[prost(uint64, tag = "1")]
    pub start_version: u64,
    #[prost(uint64, tag = "2")]
    pub end_version: u64,
    #[prost(int64, tag = "3")]
    pub timestamp_ms: i64,
    #[prost(message, repeated, tag = "4")]
    pub transactions: Vec<ProtoTransaction>,
}

impl FileSource {
    /// Create a new file-based source from the given path and replay speed (1.0 = real-time).
    pub fn new(path: String, replay_speed: f64) -> Result<Self> {
        let data = fs::read(path)?;
        Ok(Self {
            buf: Bytes::from(data),
            first_timestamp_ms: None,
            start_instant: Instant::now(),
            replay_speed,
        })
    }
}

#[async_trait]
impl DataSource for FileSource {
    async fn get_next_batch(&mut self) -> Result<TransactionsPBResponse> {
        // Decode the next length-delimited RecordedBatch from the file
        let batch = RecordedBatch::decode_length_delimited(&mut self.buf)?;

        // Manage replay timing based on recorded timestamps
        if let Some(first_ts) = self.first_timestamp_ms {
            let elapsed_ms = (batch.timestamp_ms - first_ts).max(0) as u64;
            let delay = Duration::from_millis((elapsed_ms as f64 / self.replay_speed) as u64);
            let target = self.start_instant + delay;
            let now = Instant::now();
            if target > now {
                tokio::time::sleep(target - now).await;
            }
        } else {
            self.first_timestamp_ms = Some(batch.timestamp_ms);
            self.start_instant = Instant::now();
        }

        // Prepare TransactionsPBResponse from recorded data
        let RecordedBatch {
            start_version,
            end_version,
            transactions,
            ..
        } = batch;
        // Recorded data does not include a chain_id; default to zero
        let chain_id = 0;
        let start_txn_timestamp = transactions.first().and_then(|t| t.timestamp);
        let end_txn_timestamp = transactions.last().and_then(|t| t.timestamp);
        // Size is unknown for replay; set to zero
        let size_in_bytes = 0;

        Ok(TransactionsPBResponse {
            transactions,
            chain_id,
            start_version,
            end_version,
            start_txn_timestamp,
            end_txn_timestamp,
            size_in_bytes,
        })
    }
}