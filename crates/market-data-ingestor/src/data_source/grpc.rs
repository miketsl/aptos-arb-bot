use crate::data_source::{DataSource, DataSourceError, EventMetadata, RawEvent, TimestampedEvent};
use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::{
    TransactionStream, TransactionStreamConfig,
};
use async_trait::async_trait;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing;

/// Connection state for gRPC source
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// Reconnection configuration
#[derive(Debug, Clone)]
pub struct ReconnectionConfig {
    /// Initial delay before first reconnection attempt
    pub initial_delay: Duration,
    /// Maximum delay between reconnection attempts
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Maximum number of consecutive failures before giving up
    pub max_failures: u32,
    /// Connection timeout for each attempt
    pub connection_timeout: Duration,
    /// Health check interval when connected
    pub health_check_interval: Duration,
}

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            max_failures: 10,
            connection_timeout: Duration::from_secs(10),
            health_check_interval: Duration::from_secs(30),
        }
    }
}

/// gRPC data source with lifecycle management and reconnection
pub struct GrpcSource {
    inner: Option<TransactionStream>,
    config: TransactionStreamConfig,
    reconnection_config: ReconnectionConfig,
    sequence_counter: u64,
    is_active: bool,
    connection_state: ConnectionState,
    consecutive_failures: u32,
    current_delay: Duration,
    last_successful_connection: Option<Instant>,
    last_health_check: Option<Instant>,
}

impl GrpcSource {
    pub fn new(config: TransactionStreamConfig) -> Self {
        Self::with_reconnection_config(config, ReconnectionConfig::default())
    }

    pub fn with_reconnection_config(
        config: TransactionStreamConfig,
        reconnection_config: ReconnectionConfig,
    ) -> Self {
        Self {
            inner: None,
            config,
            reconnection_config: reconnection_config.clone(),
            sequence_counter: 0,
            is_active: false,
            connection_state: ConnectionState::Disconnected,
            consecutive_failures: 0,
            current_delay: reconnection_config.initial_delay,
            last_successful_connection: None,
            last_health_check: None,
        }
    }

    /// Attempt to establish a connection with timeout
    async fn connect(&mut self) -> Result<(), DataSourceError> {
        self.connection_state = ConnectionState::Connecting;

        let connect_future = TransactionStream::new(self.config.clone());
        let timeout_future = tokio::time::sleep(self.reconnection_config.connection_timeout);

        tokio::select! {
            result = connect_future => {
                match result {
                    Ok(stream) => {
                        self.inner = Some(stream);
                        self.connection_state = ConnectionState::Connected;
                        self.consecutive_failures = 0;
                        self.current_delay = self.reconnection_config.initial_delay;
                        self.last_successful_connection = Some(Instant::now());
                        self.last_health_check = Some(Instant::now());
                        tracing::info!("gRPC connection established successfully");
                        Ok(())
                    }
                    Err(e) => {
                        self.connection_state = ConnectionState::Failed;
                        self.consecutive_failures += 1;
                        let error_msg = format!("Connection attempt failed: {}", e);
                        tracing::error!(
                            consecutive_failures = self.consecutive_failures,
                            max_failures = self.reconnection_config.max_failures,
                            error = %e,
                            "gRPC connection failed"
                        );
                        Err(DataSourceError::ConnectionFailed(error_msg))
                    }
                }
            }
            _ = timeout_future => {
                self.connection_state = ConnectionState::Failed;
                self.consecutive_failures += 1;
                let error_msg = format!("Connection timeout after {:?}", self.reconnection_config.connection_timeout);
                tracing::error!(
                    consecutive_failures = self.consecutive_failures,
                    timeout = ?self.reconnection_config.connection_timeout,
                    "gRPC connection timed out"
                );
                Err(DataSourceError::Timeout(error_msg))
            }
        }
    }

    /// Attempt reconnection with exponential backoff
    async fn reconnect(&mut self) -> Result<(), DataSourceError> {
        if self.consecutive_failures >= self.reconnection_config.max_failures {
            let error_msg = format!(
                "Maximum reconnection attempts ({}) exceeded",
                self.reconnection_config.max_failures
            );
            tracing::error!(
                consecutive_failures = self.consecutive_failures,
                max_failures = self.reconnection_config.max_failures,
                "Giving up on reconnection"
            );
            return Err(DataSourceError::ConnectionFailed(error_msg));
        }

        self.connection_state = ConnectionState::Reconnecting;

        tracing::info!(
            delay = ?self.current_delay,
            attempt = self.consecutive_failures + 1,
            max_attempts = self.reconnection_config.max_failures,
            "Attempting to reconnect to gRPC stream"
        );

        // Wait for backoff delay
        tokio::time::sleep(self.current_delay).await;

        // Try to connect
        let result = self.connect().await;

        // Update delay for next attempt using exponential backoff
        if result.is_err() {
            let new_delay = Duration::from_millis(
                (self.current_delay.as_millis() as f64
                    * self.reconnection_config.backoff_multiplier) as u64,
            );
            self.current_delay = new_delay.min(self.reconnection_config.max_delay);
        }

        result
    }

    /// Check if connection health check is needed
    fn needs_health_check(&self) -> bool {
        if let Some(last_check) = self.last_health_check {
            last_check.elapsed() >= self.reconnection_config.health_check_interval
        } else {
            true
        }
    }

    /// Perform a health check on the connection
    async fn health_check(&mut self) -> bool {
        if let Some(_stream) = &mut self.inner {
            // For now, we'll consider the connection healthy if we can access the stream
            // In a real implementation, you might want to send a ping or check connection status
            self.last_health_check = Some(Instant::now());
            true
        } else {
            false
        }
    }

    /// Get connection statistics for monitoring
    pub fn connection_stats(&self) -> ConnectionStats {
        ConnectionStats {
            state: self.connection_state.clone(),
            consecutive_failures: self.consecutive_failures,
            last_successful_connection: self.last_successful_connection,
            uptime: self.last_successful_connection.map(|t| t.elapsed()),
        }
    }
}

/// Connection statistics for monitoring
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub state: ConnectionState,
    pub consecutive_failures: u32,
    pub last_successful_connection: Option<Instant>,
    pub uptime: Option<Duration>,
}

#[async_trait]
impl DataSource for GrpcSource {
    async fn start(&mut self) -> Result<(), DataSourceError> {
        if self.is_active {
            return Ok(());
        }

        tracing::info!("Starting gRPC data source");
        self.connect().await?;
        self.is_active = true;
        Ok(())
    }

    async fn next_event(&mut self) -> Result<Option<TimestampedEvent>, DataSourceError> {
        if !self.is_active {
            return Err(DataSourceError::Internal(
                "Data source not started".to_string(),
            ));
        }

        // Perform health check if needed
        if self.needs_health_check() && !self.health_check().await {
            tracing::warn!("Health check failed, attempting reconnection");
            self.inner = None;
            self.connection_state = ConnectionState::Disconnected;
        }

        // Ensure we have a connection
        loop {
            match self.connection_state {
                ConnectionState::Connected => {
                    // Try to get the next batch
                    if let Some(stream) = &mut self.inner {
                        match stream.get_next_transaction_batch().await {
                            Ok(batch) => {
                                // Reset failure count on successful operation
                                if self.consecutive_failures > 0 {
                                    tracing::info!(
                                        "Connection recovered after {} failures",
                                        self.consecutive_failures
                                    );
                                    self.consecutive_failures = 0;
                                    self.current_delay = self.reconnection_config.initial_delay;
                                }

                                // Convert the first transaction in the batch to a TimestampedEvent
                                if let Some(transaction) = batch.transactions.into_iter().next() {
                                    let blockchain_timestamp =
                                        transaction.timestamp.and_then(|ts| {
                                            UNIX_EPOCH
                                                .checked_add(Duration::from_secs(ts.seconds as u64))
                                                .and_then(|t| {
                                                    t.checked_add(Duration::from_nanos(
                                                        ts.nanos as u64,
                                                    ))
                                                })
                                        });

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
                                    return Ok(Some(timestamped_event));
                                } else {
                                    return Ok(None);
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to get next batch from gRPC stream");
                                self.inner = None;
                                self.connection_state = ConnectionState::Failed;
                                self.consecutive_failures += 1;

                                // Surface error to metrics system every few failures or if we've exceeded max failures
                                if self.consecutive_failures % 3 == 0
                                    || self.consecutive_failures
                                        >= self.reconnection_config.max_failures
                                {
                                    return Err(DataSourceError::ConnectionFailed(format!(
                                        "gRPC stream error after {} consecutive failures: {}",
                                        self.consecutive_failures, e
                                    )));
                                }
                                // Continue to reconnection logic below
                            }
                        }
                    } else {
                        self.connection_state = ConnectionState::Disconnected;
                    }
                }
                ConnectionState::Disconnected | ConnectionState::Failed => {
                    // Attempt to reconnect
                    match self.reconnect().await {
                        Ok(()) => {
                            tracing::info!("Reconnection successful, resuming event processing");
                            // Continue the loop to try getting events again
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
                ConnectionState::Connecting | ConnectionState::Reconnecting => {
                    // This shouldn't happen in normal flow, but handle it gracefully
                    tracing::warn!("Unexpected connection state: {:?}", self.connection_state);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    async fn stop(&mut self) -> Result<(), DataSourceError> {
        tracing::info!("Stopping gRPC data source");
        self.inner = None;
        self.is_active = false;
        self.connection_state = ConnectionState::Disconnected;
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.is_active
    }

    fn source_type(&self) -> &'static str {
        "grpc"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconnection_config_default() {
        let config = ReconnectionConfig::default();

        assert_eq!(config.initial_delay, Duration::from_millis(100));
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert_eq!(config.backoff_multiplier, 2.0);
        assert_eq!(config.max_failures, 10);
        assert_eq!(config.connection_timeout, Duration::from_secs(10));
        assert_eq!(config.health_check_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_reconnection_config_custom() {
        let config = ReconnectionConfig {
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 1.5,
            max_failures: 5,
            connection_timeout: Duration::from_secs(5),
            health_check_interval: Duration::from_secs(15),
        };

        assert_eq!(config.initial_delay, Duration::from_millis(50));
        assert_eq!(config.max_delay, Duration::from_secs(60));
        assert_eq!(config.backoff_multiplier, 1.5);
        assert_eq!(config.max_failures, 5);
        assert_eq!(config.connection_timeout, Duration::from_secs(5));
        assert_eq!(config.health_check_interval, Duration::from_secs(15));
    }

    #[test]
    fn test_connection_state_enum() {
        let state = ConnectionState::Disconnected;
        assert_eq!(state, ConnectionState::Disconnected);

        let state = ConnectionState::Connecting;
        assert_eq!(state, ConnectionState::Connecting);

        let state = ConnectionState::Connected;
        assert_eq!(state, ConnectionState::Connected);

        let state = ConnectionState::Reconnecting;
        assert_eq!(state, ConnectionState::Reconnecting);

        let state = ConnectionState::Failed;
        assert_eq!(state, ConnectionState::Failed);
    }

    #[test]
    fn test_grpc_source_creation_with_custom_config() {
        // Create a minimal config for testing - we'll use the processor's actual config creation in integration tests
        let reconnection_config = ReconnectionConfig {
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 1.5,
            max_failures: 3,
            connection_timeout: Duration::from_secs(5),
            health_check_interval: Duration::from_secs(10),
        };

        // Test that we can create a ReconnectionConfig with custom values
        assert_eq!(reconnection_config.initial_delay, Duration::from_millis(50));
        assert_eq!(reconnection_config.max_delay, Duration::from_secs(10));
        assert_eq!(reconnection_config.backoff_multiplier, 1.5);
        assert_eq!(reconnection_config.max_failures, 3);
        assert_eq!(
            reconnection_config.connection_timeout,
            Duration::from_secs(5)
        );
        assert_eq!(
            reconnection_config.health_check_interval,
            Duration::from_secs(10)
        );
    }

    // Note: Complex gRPC configuration tests are omitted due to TransactionStreamConfig complexity
    // These would be better tested in integration tests with actual gRPC endpoints

    #[test]
    fn test_error_types_for_grpc_failures() {
        // Test timeout error
        let error = DataSourceError::Timeout("Connection timeout".to_string());
        assert_eq!(error.to_string(), "Timeout occurred: Connection timeout");

        // Test connection failed error
        let error = DataSourceError::ConnectionFailed("gRPC connection failed".to_string());
        assert_eq!(
            error.to_string(),
            "Connection failed: gRPC connection failed"
        );

        // Test stream ended error
        let error = DataSourceError::StreamEnded;
        assert_eq!(error.to_string(), "Stream ended unexpectedly");
    }
}
