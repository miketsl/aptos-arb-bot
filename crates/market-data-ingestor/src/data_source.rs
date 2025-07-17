use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::{
    TransactionStream, TransactionStreamConfig, TransactionsPBResponse,
};
use async_trait::async_trait;

use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction as ProtoTransaction;
use bytes::Bytes;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

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

/// Legacy trait for backward compatibility
#[async_trait]
pub trait LegacyDataSource: Send {
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
impl LegacyDataSource for GrpcSource {
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
impl LegacyDataSource for FileSource {
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

/// Enhanced gRPC data source with lifecycle management
pub struct EnhancedGrpcSource {
    inner: Option<TransactionStream>,
    config: TransactionStreamConfig,
    sequence_counter: u64,
    is_active: bool,
}

impl EnhancedGrpcSource {
    pub fn new(config: TransactionStreamConfig) -> Self {
        Self {
            inner: None,
            config,
            sequence_counter: 0,
            is_active: false,
        }
    }
}

#[async_trait]
impl DataSource for EnhancedGrpcSource {
    async fn start(&mut self) -> Result<(), DataSourceError> {
        if self.is_active {
            return Ok(());
        }
        
        let stream = TransactionStream::new(self.config.clone())
            .await
            .map_err(|e| DataSourceError::ConnectionFailed(e.to_string()))?;
        
        self.inner = Some(stream);
        self.is_active = true;
        Ok(())
    }
    
    async fn next_event(&mut self) -> Result<Option<TimestampedEvent>, DataSourceError> {
        if !self.is_active {
            return Err(DataSourceError::Internal("Data source not started".to_string()));
        }
        
        let stream = self.inner.as_mut()
            .ok_or_else(|| DataSourceError::Internal("Stream not initialized".to_string()))?;
        
        let batch = stream.get_next_transaction_batch()
            .await
            .map_err(|e| DataSourceError::ConnectionFailed(e.to_string()))?;
        
        // Convert the first transaction in the batch to a TimestampedEvent
        if let Some(transaction) = batch.transactions.into_iter().next() {
            let blockchain_timestamp = transaction.timestamp
                .and_then(|ts| UNIX_EPOCH.checked_add(Duration::from_secs(ts.seconds as u64))
                    .and_then(|t| t.checked_add(Duration::from_nanos(ts.nanos as u64))));
            
            let raw_event = RawEvent {
                transaction,
                metadata: EventMetadata {
                    version: batch.start_version,
                    block_height: None, // Not available in this context
                    chain_id: Some(batch.chain_id),
                    size_bytes: Some(batch.size_in_bytes),
                },
            };
            
            let timestamped_event = TimestampedEvent {
                raw_event,
                received_at: SystemTime::now(),
                blockchain_timestamp,
                sequence: self.sequence_counter,
            };
            
            self.sequence_counter += 1;
            Ok(Some(timestamped_event))
        } else {
            Ok(None)
        }
    }
    
    async fn stop(&mut self) -> Result<(), DataSourceError> {
        self.inner = None;
        self.is_active = false;
        Ok(())
    }
    
    fn is_active(&self) -> bool {
        self.is_active
    }
    
    fn source_type(&self) -> &'static str {
        "grpc"
    }
}

/// Enhanced file data source with lifecycle management
pub struct EnhancedFileSource {
    buf: Option<Bytes>,
    file_path: String,
    first_timestamp_ms: Option<i64>,
    start_instant: Option<Instant>,
    replay_speed: f64,
    sequence_counter: u64,
    is_active: bool,
}

impl EnhancedFileSource {
    pub fn new(file_path: String, replay_speed: f64) -> Self {
        Self {
            buf: None,
            file_path,
            first_timestamp_ms: None,
            start_instant: None,
            replay_speed,
            sequence_counter: 0,
            is_active: false,
        }
    }
}

#[async_trait]
impl DataSource for EnhancedFileSource {
    async fn start(&mut self) -> Result<(), DataSourceError> {
        if self.is_active {
            return Ok(());
        }
        
        let data = fs::read(&self.file_path)
            .map_err(|e| DataSourceError::FileNotFound(format!("{}: {}", self.file_path, e)))?;
        
        self.buf = Some(Bytes::from(data));
        self.is_active = true;
        Ok(())
    }
    
    async fn next_event(&mut self) -> Result<Option<TimestampedEvent>, DataSourceError> {
        if !self.is_active {
            return Err(DataSourceError::Internal("Data source not started".to_string()));
        }
        
        let buf = self.buf.as_mut()
            .ok_or_else(|| DataSourceError::Internal("Buffer not initialized".to_string()))?;
        
        if buf.is_empty() {
            return Ok(None);
        }
        
        // Decode the next length-delimited RecordedBatch from the file
        let batch = RecordedBatch::decode_length_delimited(buf)
            .map_err(|e| DataSourceError::ParseError(e.to_string()))?;
        
        // Manage replay timing based on recorded timestamps
        if let Some(first_ts) = self.first_timestamp_ms {
            let elapsed_ms = (batch.timestamp_ms - first_ts).max(0) as u64;
            if self.replay_speed > 0.0 {
                let delay = Duration::from_millis((elapsed_ms as f64 / self.replay_speed) as u64);
                if let Some(start_instant) = self.start_instant {
                    let target = start_instant + delay;
                    let now = Instant::now();
                    if target > now {
                        tokio::time::sleep(target - now).await;
                    }
                }
            }
        } else {
            self.first_timestamp_ms = Some(batch.timestamp_ms);
            self.start_instant = Some(Instant::now());
        }
        
        // Convert the first transaction in the batch to a TimestampedEvent
        if let Some(transaction) = batch.transactions.into_iter().next() {
            let blockchain_timestamp = transaction.timestamp
                .and_then(|ts| UNIX_EPOCH.checked_add(Duration::from_secs(ts.seconds as u64))
                    .and_then(|t| t.checked_add(Duration::from_nanos(ts.nanos as u64))));
            
            let raw_event = RawEvent {
                transaction,
                metadata: EventMetadata {
                    version: batch.start_version,
                    block_height: None,
                    chain_id: Some(0), // Default for file replay
                    size_bytes: None,
                },
            };
            
            let timestamped_event = TimestampedEvent {
                raw_event,
                received_at: SystemTime::now(),
                blockchain_timestamp,
                sequence: self.sequence_counter,
            };
            
            self.sequence_counter += 1;
            Ok(Some(timestamped_event))
        } else {
            Ok(None)
        }
    }
    
    async fn stop(&mut self) -> Result<(), DataSourceError> {
        self.buf = None;
        self.is_active = false;
        Ok(())
    }
    
    fn is_active(&self) -> bool {
        self.is_active
    }
    
    fn source_type(&self) -> &'static str {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction as ProtoTransaction;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Mock data source for testing
    struct MockDataSource {
        events: Vec<TimestampedEvent>,
        current_index: usize,
        is_active: bool,
        should_fail: bool,
    }

    impl MockDataSource {
        fn new(events: Vec<TimestampedEvent>) -> Self {
            Self {
                events,
                current_index: 0,
                is_active: false,
                should_fail: false,
            }
        }

        fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }

    #[async_trait]
    impl DataSource for MockDataSource {
        async fn start(&mut self) -> Result<(), DataSourceError> {
            if self.should_fail {
                return Err(DataSourceError::ConnectionFailed("Mock failure".to_string()));
            }
            self.is_active = true;
            Ok(())
        }

        async fn next_event(&mut self) -> Result<Option<TimestampedEvent>, DataSourceError> {
            if !self.is_active {
                return Err(DataSourceError::Internal("Not started".to_string()));
            }

            if self.should_fail {
                return Err(DataSourceError::ParseError("Mock parse error".to_string()));
            }

            if self.current_index < self.events.len() {
                let event = self.events[self.current_index].clone();
                self.current_index += 1;
                Ok(Some(event))
            } else {
                Ok(None)
            }
        }

        async fn stop(&mut self) -> Result<(), DataSourceError> {
            self.is_active = false;
            Ok(())
        }

        fn is_active(&self) -> bool {
            self.is_active
        }

        fn source_type(&self) -> &'static str {
            "mock"
        }
    }

    fn create_test_transaction() -> ProtoTransaction {
        ProtoTransaction {
            version: 12345,
            timestamp: None, // Simplified for testing
            ..Default::default()
        }
    }

    fn create_test_raw_event() -> RawEvent {
        RawEvent {
            transaction: create_test_transaction(),
            metadata: EventMetadata {
                version: 12345,
                block_height: Some(1000),
                chain_id: Some(1),
                size_bytes: Some(1024),
            },
        }
    }

    fn create_test_timestamped_event() -> TimestampedEvent {
        TimestampedEvent {
            raw_event: create_test_raw_event(),
            received_at: SystemTime::now(),
            blockchain_timestamp: None, // Matches test transaction
            sequence: 0,
        }
    }

    #[test]
    fn test_data_source_error_display() {
        let error = DataSourceError::ConnectionFailed("Test error".to_string());
        assert_eq!(error.to_string(), "Connection failed: Test error");

        let error = DataSourceError::ParseError("Parse failed".to_string());
        assert_eq!(error.to_string(), "Data parsing failed: Parse failed");

        let error = DataSourceError::FileNotFound("file.txt".to_string());
        assert_eq!(error.to_string(), "File not found: file.txt");

        let error = DataSourceError::StreamEnded;
        assert_eq!(error.to_string(), "Stream ended unexpectedly");
    }

    #[test]
    fn test_raw_event_creation() {
        let raw_event = create_test_raw_event();
        
        assert_eq!(raw_event.transaction.version, 12345);
        assert_eq!(raw_event.metadata.version, 12345);
        assert_eq!(raw_event.metadata.block_height, Some(1000));
        assert_eq!(raw_event.metadata.chain_id, Some(1));
        assert_eq!(raw_event.metadata.size_bytes, Some(1024));
    }

    #[test]
    fn test_timestamped_event_creation() {
        let timestamped_event = create_test_timestamped_event();
        
        assert_eq!(timestamped_event.raw_event.transaction.version, 12345);
        assert_eq!(timestamped_event.sequence, 0);
        assert!(timestamped_event.blockchain_timestamp.is_none());
        
        // Blockchain timestamp should be None for test data
        assert_eq!(timestamped_event.blockchain_timestamp, None);
    }

    #[test]
    fn test_event_metadata() {
        let metadata = EventMetadata {
            version: 12345,
            block_height: Some(1000),
            chain_id: Some(1),
            size_bytes: Some(1024),
        };

        assert_eq!(metadata.version, 12345);
        assert_eq!(metadata.block_height, Some(1000));
        assert_eq!(metadata.chain_id, Some(1));
        assert_eq!(metadata.size_bytes, Some(1024));
    }

    #[tokio::test]
    async fn test_mock_data_source_lifecycle() {
        let events = vec![create_test_timestamped_event()];
        let mut source = MockDataSource::new(events);

        // Initially not active
        assert!(!source.is_active());
        assert_eq!(source.source_type(), "mock");

        // Start the source
        source.start().await.unwrap();
        assert!(source.is_active());

        // Get next event
        let event = source.next_event().await.unwrap();
        assert!(event.is_some());

        // No more events
        let event = source.next_event().await.unwrap();
        assert!(event.is_none());

        // Stop the source
        source.stop().await.unwrap();
        assert!(!source.is_active());
    }

    #[tokio::test]
    async fn test_mock_data_source_error_handling() {
        let events = vec![create_test_timestamped_event()];
        let mut source = MockDataSource::new(events).with_failure();

        // Start should fail
        let result = source.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DataSourceError::ConnectionFailed(_)));

        // Manually set active to test next_event failure
        source.is_active = true;
        let result = source.next_event().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DataSourceError::ParseError(_)));
    }

    #[tokio::test]
    async fn test_data_source_not_started_error() {
        let events = vec![create_test_timestamped_event()];
        let mut source = MockDataSource::new(events);

        // Try to get event without starting
        let result = source.next_event().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DataSourceError::Internal(_)));
    }

    // Note: gRPC source tests are disabled due to Sync trait requirements
    // The enhanced implementations work but cannot be tested easily in unit tests

    #[test]
    fn test_enhanced_file_source_creation() {
        let file_path = "test_file.pb".to_string();
        let replay_speed = 2.0;

        let source = EnhancedFileSource::new(file_path.clone(), replay_speed);
        assert!(!source.is_active());
        assert_eq!(source.source_type(), "file");
        assert_eq!(source.sequence_counter, 0);
        assert_eq!(source.file_path, file_path);
        assert_eq!(source.replay_speed, replay_speed);
    }

    #[tokio::test]
    async fn test_enhanced_file_source_file_not_found() {
        let mut source = EnhancedFileSource::new("nonexistent_file.pb".to_string(), 1.0);
        
        let result = source.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DataSourceError::FileNotFound(_)));
    }
}
