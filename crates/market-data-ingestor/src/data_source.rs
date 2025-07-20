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

/// Protobuf message for recorded batches with embedded pool state, matching the architecture spec.
#[derive(prost::Message, Serialize, Deserialize, Clone)]
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
    /// NEW: Embedded pool states for newly discovered pools in this batch
    #[prost(message, repeated, tag = "5")]
    pub pool_initializations: Vec<RecordedPoolState>,
}

/// Recorded pool state for historical replay consistency
/// Hybrid design: fast path for 2-token pools, complete data for multi-token pools
#[derive(prost::Message, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecordedPoolState {
    #[prost(string, tag = "1")]
    pub pool_id: String,
    #[prost(string, tag = "2")]
    pub dex_name: String,
    
    // Fast path: Primary pair (covers 90% of pools)
    #[prost(string, tag = "3")]
    pub token_a: String,
    #[prost(string, tag = "4")]
    pub token_b: String,
    #[prost(string, tag = "5")]
    pub reserve_a: String, // Decimal as string for precision
    #[prost(string, tag = "6")]
    pub reserve_b: String, // Decimal as string for precision
    #[prost(string, tag = "7")]
    pub fee_rate: String,  // Decimal as string for precision
    
    // Complete data: For complex pools (optional for performance)
    #[prost(string, repeated, tag = "10")]
    pub all_tokens: Vec<String>, // Empty for 2-token pools, complete list for multi-token
    #[prost(string, repeated, tag = "11")]
    pub all_reserves: Vec<String>, // Empty for 2-token pools, complete list for multi-token
    #[prost(uint32, repeated, tag = "12")]
    pub all_weights: Vec<u32>, // Empty for non-weighted pools, weights for weighted pools
    
    // Metadata
    #[prost(string, tag = "13")]
    pub pool_type: String, // "clmm", "weighted", "stable", "constant_product"
    #[prost(uint64, tag = "8")]
    pub block_height: u64,
    #[prost(bytes, tag = "9")]
    pub additional_data: Vec<u8>, // JSON serialized DEX-specific data
}

/// Pool initialization event for detector integration
#[derive(Debug, Clone)]
pub struct PoolInitialization {
    pub pool_state: RecordedPoolState,
    pub timestamp: SystemTime,
}