use crate::data_source::{DataSource, DataSourceError, RawEvent, TimestampedEvent, EventMetadata, RecordedBatch};
use async_trait::async_trait;
use bytes::Bytes;
use prost::Message;
use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// File data source with lifecycle management
pub struct FileSource {
    buf: Option<Bytes>,
    file_path: String,
    first_timestamp_ms: Option<i64>,
    start_instant: Option<Instant>,
    replay_speed: f64,
    sequence_counter: u64,
    is_active: bool,
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
        }
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
    }

    #[tokio::test]
    async fn test_file_source_file_not_found() {
        let mut source = FileSource::new("nonexistent_file.pb".to_string(), 1.0);
        
        let result = source.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DataSourceError::FileNotFound(_)));
    }
}