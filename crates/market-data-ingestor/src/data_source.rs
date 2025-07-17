// Re-export everything from the modular structure
pub mod grpc;
pub mod file;

// Re-export core types and traits
use async_trait::async_trait;
use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction as ProtoTransaction;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use thiserror::Error;

pub use grpc::{GrpcSource, ReconnectionConfig, ConnectionStats, ConnectionState};
pub use file::FileSource;

/// Error types for data source operations
#[derive(Error, Debug)]
pub enum DataSourceError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Data parsing failed: {0}")]
    ParseError(String),
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Stream ended unexpectedly")]
    StreamEnded,
    #[error("Timeout occurred: {0}")]
    Timeout(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Raw event data from the data source
#[derive(Debug, Clone)]
pub struct RawEvent {
    /// The raw transaction data
    pub transaction: ProtoTransaction,
    /// Source-specific metadata
    pub metadata: EventMetadata,
}

/// Timestamped event with processing metadata
#[derive(Debug, Clone)]
pub struct TimestampedEvent {
    /// The raw event data
    pub raw_event: RawEvent,
    /// When this event was received by the data source
    pub received_at: SystemTime,
    /// Original timestamp from the blockchain
    pub blockchain_timestamp: Option<SystemTime>,
    /// Sequence number for ordering
    pub sequence: u64,
}

/// Metadata associated with an event
#[derive(Debug, Clone)]
pub struct EventMetadata {
    /// Version number of the transaction
    pub version: u64,
    /// Block height
    pub block_height: Option<u64>,
    /// Chain ID
    pub chain_id: Option<u64>,
    /// Size in bytes
    pub size_bytes: Option<u64>,
}

/// Enhanced data source trait with lifecycle management
#[async_trait]
pub trait DataSource: Send {
    /// Start the data source and begin producing events
    async fn start(&mut self) -> Result<(), DataSourceError>;
    
    /// Get the next event from the data source
    async fn next_event(&mut self) -> Result<Option<TimestampedEvent>, DataSourceError>;
    
    /// Stop the data source and clean up resources
    async fn stop(&mut self) -> Result<(), DataSourceError>;
    
    /// Check if the data source is currently active
    fn is_active(&self) -> bool;
    
    /// Get the name/type of this data source for logging
    fn source_type(&self) -> &'static str;
}

/// Protobuf message for recorded batches, matching the architecture spec.
#[derive(prost::Message, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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