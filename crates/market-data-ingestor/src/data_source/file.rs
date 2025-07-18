use crate::data_source::{DataSource, DataSourceError, RawEvent, TimestampedEvent, EventMetadata, RecordedBatch, RecordedPoolState, PoolInitialization};
use async_trait::async_trait;
use bytes::Bytes;
use prost::Message;
use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing;

/// File data source with lifecycle management and embedded pool state support
pub struct FileSource {
    buf: Option<Bytes>,
    file_path: String,
    first_timestamp_ms: Option<i64>,
    start_instant: Option<Instant>,
    replay_speed: f64,
    sequence_counter: u64,
    is_active: bool,
    /// Pool initializations from the current batch that need to be processed
    pending_pool_initializations: Vec<PoolInitialization>,
    /// Current batch being processed (for multi-transaction batches)
    current_batch: Option<RecordedBatch>,
    /// Index of current transaction within the batch
    current_transaction_index: usize,
}

impl FileSource {
    pub fn new(file_path: String, replay_speed: f64) -> Self {
        Self {
            buf: None,
            file_path,
            first_timestamp_ms: None,
            start_instant: None,
            replay_speed,
            sequence_counter: 0,
            is_active: false,
            pending_pool_initializations: Vec::new(),
            current_batch: None,
            current_transaction_index: 0,
        }
    }

    /// Check if fast-forward mode is enabled (replay_speed = 0.0)
    pub fn is_fast_forward_mode(&self) -> bool {
        self.replay_speed == 0.0
    }

    /// Get pending pool initializations that need to be processed
    pub fn get_pending_pool_initializations(&mut self) -> Vec<PoolInitialization> {
        std::mem::take(&mut self.pending_pool_initializations)
    }

    /// Process embedded pool states from a batch
    fn process_pool_initializations(&mut self, pool_states: Vec<RecordedPoolState>, batch_timestamp: SystemTime) {
        for pool_state in pool_states {
            let pool_init = PoolInitialization {
                pool_state,
                timestamp: batch_timestamp,
            };
            self.pending_pool_initializations.push(pool_init);
        }
    }

    /// Handle replay timing based on recorded timestamps
    async fn handle_replay_timing(&mut self, batch_timestamp_ms: i64) {
        if let Some(first_ts) = self.first_timestamp_ms {
            let elapsed_ms = (batch_timestamp_ms - first_ts).max(0) as u64;
            let delay = Duration::from_millis((elapsed_ms as f64 / self.replay_speed) as u64);
            if let Some(start_instant) = self.start_instant {
                let target = start_instant + delay;
                let now = Instant::now();
                if target > now {
                    tokio::time::sleep(target - now).await;
                }
            }
        } else {
            self.first_timestamp_ms = Some(batch_timestamp_ms);
            self.start_instant = Some(Instant::now());
        }
    }

    /// Create a TimestampedEvent from a transaction
    fn create_timestamped_event(&mut self, transaction: aptos_indexer_processor_sdk::aptos_protos::transaction::v1::Transaction, start_version: u64) -> Result<Option<TimestampedEvent>, DataSourceError> {
        let blockchain_timestamp = transaction.timestamp
            .and_then(|ts| UNIX_EPOCH.checked_add(Duration::from_secs(ts.seconds as u64))
                .and_then(|t| t.checked_add(Duration::from_nanos(ts.nanos as u64))));
        
        let raw_event = RawEvent {
            transaction,
            metadata: EventMetadata {
                version: start_version,
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
    }
}

#[async_trait]
impl DataSource for FileSource {
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

        // If we have a current batch being processed, continue with it
        if let Some(ref batch) = self.current_batch.clone() {
            if self.current_transaction_index < batch.transactions.len() {
                let transaction = batch.transactions[self.current_transaction_index].clone();
                self.current_transaction_index += 1;

                // If this is the last transaction in the batch, clear the current batch
                if self.current_transaction_index >= batch.transactions.len() {
                    self.current_batch = None;
                    self.current_transaction_index = 0;
                }

                return self.create_timestamped_event(transaction, batch.start_version);
            }
        }

        // Need to load the next batch
        let buf = self.buf.as_mut()
            .ok_or_else(|| DataSourceError::Internal("Buffer not initialized".to_string()))?;
        
        if buf.is_empty() {
            return Ok(None);
        }
        
        // Decode the next length-delimited RecordedBatch from the file
        let batch = RecordedBatch::decode_length_delimited(buf)
            .map_err(|e| DataSourceError::ParseError(e.to_string()))?;
        
        // Handle timing for replay speed (skip if fast-forward mode)
        if !self.is_fast_forward_mode() {
            self.handle_replay_timing(batch.timestamp_ms).await;
        }

        // Process embedded pool states first
        if !batch.pool_initializations.is_empty() {
            let batch_timestamp = SystemTime::now(); // Use current time for pool initialization
            self.process_pool_initializations(batch.pool_initializations.clone(), batch_timestamp);
            tracing::info!(
                pool_count = batch.pool_initializations.len(),
                batch_start_version = batch.start_version,
                "Processed embedded pool states from recorded batch"
            );
        }

        // Process transactions from the batch
        if batch.transactions.is_empty() {
            return Ok(None);
        }

        // If batch has multiple transactions, store it for subsequent calls
        if batch.transactions.len() > 1 {
            let first_transaction = batch.transactions[0].clone();
            self.current_batch = Some(batch.clone());
            self.current_transaction_index = 1; // Next call will get index 1
            
            return self.create_timestamped_event(first_transaction, batch.start_version);
        } else {
            // Single transaction batch - process immediately
            let transaction = batch.transactions[0].clone();
            return self.create_timestamped_event(transaction, batch.start_version);
        }
    }
    
    async fn stop(&mut self) -> Result<(), DataSourceError> {
        self.buf = None;
        self.is_active = false;
        self.pending_pool_initializations.clear();
        self.current_batch = None;
        self.current_transaction_index = 0;
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
    use prost::Message;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_recorded_batch_with_pools() -> RecordedBatch {
        let pool_state = RecordedPoolState {
            pool_id: "test_pool_123".to_string(),
            dex_name: "hyperion".to_string(),
            token_a: "APT".to_string(),
            token_b: "USDC".to_string(),
            reserve_a: "1000000".to_string(),
            reserve_b: "500000".to_string(),
            fee_rate: "0.003".to_string(),
            block_height: 12345,
            additional_data: b"{\"tick_spacing\": 100}".to_vec(),
        };

        RecordedBatch {
            start_version: 12345,
            end_version: 12346,
            timestamp_ms: 1640995200000, // 2022-01-01 00:00:00 UTC
            transactions: vec![ProtoTransaction {
                version: 12345,
                timestamp: None,
                ..Default::default()
            }],
            pool_initializations: vec![pool_state],
        }
    }

    fn create_test_recorded_batch_multi_transaction() -> RecordedBatch {
        RecordedBatch {
            start_version: 12345,
            end_version: 12347,
            timestamp_ms: 1640995200000,
            transactions: vec![
                ProtoTransaction {
                    version: 12345,
                    timestamp: None,
                    ..Default::default()
                },
                ProtoTransaction {
                    version: 12346,
                    timestamp: None,
                    ..Default::default()
                },
                ProtoTransaction {
                    version: 12347,
                    timestamp: None,
                    ..Default::default()
                },
            ],
            pool_initializations: vec![],
        }
    }

    fn create_test_file_with_batches(batches: Vec<RecordedBatch>) -> NamedTempFile {
        let mut temp_file = NamedTempFile::new().unwrap();
        
        for batch in batches {
            let mut buf = Vec::new();
            batch.encode_length_delimited(&mut buf).unwrap();
            temp_file.write_all(&buf).unwrap();
        }
        
        temp_file.flush().unwrap();
        temp_file
    }

    #[test]
    fn test_file_source_creation() {
        let file_path = "test_file.pb".to_string();
        let replay_speed = 2.0;

        let source = FileSource::new(file_path.clone(), replay_speed);
        assert!(!source.is_active());
        assert_eq!(source.source_type(), "file");
        assert_eq!(source.sequence_counter, 0);
        assert_eq!(source.file_path, file_path);
        assert_eq!(source.replay_speed, replay_speed);
        assert!(source.pending_pool_initializations.is_empty());
        assert!(source.current_batch.is_none());
        assert_eq!(source.current_transaction_index, 0);
    }

    #[test]
    fn test_fast_forward_mode() {
        let source = FileSource::new("test.pb".to_string(), 0.0);
        assert!(source.is_fast_forward_mode());

        let source = FileSource::new("test.pb".to_string(), 1.0);
        assert!(!source.is_fast_forward_mode());

        let source = FileSource::new("test.pb".to_string(), 2.5);
        assert!(!source.is_fast_forward_mode());
    }

    #[tokio::test]
    async fn test_file_source_file_not_found() {
        let mut source = FileSource::new("nonexistent_file.pb".to_string(), 1.0);
        
        let result = source.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DataSourceError::FileNotFound(_)));
    }

    #[tokio::test]
    async fn test_file_source_with_embedded_pool_states() {
        let batch = create_test_recorded_batch_with_pools();
        let temp_file = create_test_file_with_batches(vec![batch]);
        
        let mut source = FileSource::new(temp_file.path().to_string_lossy().to_string(), 0.0); // Fast-forward mode
        source.start().await.unwrap();

        // Get the first event
        let event = source.next_event().await.unwrap();
        assert!(event.is_some());

        // Check that pool initializations were processed
        let pool_inits = source.get_pending_pool_initializations();
        assert_eq!(pool_inits.len(), 1);
        
        let pool_init = &pool_inits[0];
        assert_eq!(pool_init.pool_state.pool_id, "test_pool_123");
        assert_eq!(pool_init.pool_state.dex_name, "hyperion");
        assert_eq!(pool_init.pool_state.token_a, "APT");
        assert_eq!(pool_init.pool_state.token_b, "USDC");
        assert_eq!(pool_init.pool_state.reserve_a, "1000000");
        assert_eq!(pool_init.pool_state.reserve_b, "500000");
        assert_eq!(pool_init.pool_state.fee_rate, "0.003");
        assert_eq!(pool_init.pool_state.block_height, 12345);
        assert_eq!(pool_init.pool_state.additional_data, b"{\"tick_spacing\": 100}");

        // No more events
        let event = source.next_event().await.unwrap();
        assert!(event.is_none());
    }

    #[tokio::test]
    async fn test_file_source_multi_transaction_batch() {
        let batch = create_test_recorded_batch_multi_transaction();
        let temp_file = create_test_file_with_batches(vec![batch]);
        
        let mut source = FileSource::new(temp_file.path().to_string_lossy().to_string(), 0.0);
        source.start().await.unwrap();

        // Should get 3 events from the multi-transaction batch
        let event1 = source.next_event().await.unwrap();
        assert!(event1.is_some());
        assert_eq!(event1.unwrap().sequence, 0);

        let event2 = source.next_event().await.unwrap();
        assert!(event2.is_some());
        assert_eq!(event2.unwrap().sequence, 1);

        let event3 = source.next_event().await.unwrap();
        assert!(event3.is_some());
        assert_eq!(event3.unwrap().sequence, 2);

        // No more events
        let event4 = source.next_event().await.unwrap();
        assert!(event4.is_none());
    }
}