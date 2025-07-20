pub mod data_source;
pub mod file_rotation;
pub mod ingestor_config;
pub mod pool_state_manager;
pub mod processor;
pub mod recording_config;
pub mod recording_monitor;
pub mod steps;
pub mod types;

pub use ingestor_config::IndexerProcessorConfig;
pub use processor::MarketDataIngestorProcessor;

// Export the data source abstractions
pub use data_source::{
    DataSource, DataSourceError, RawEvent, TimestampedEvent, EventMetadata,
    GrpcSource, FileSource, RecordedBatch, RecordedPoolState, PoolInitialization,
    ReconnectionConfig, ConnectionStats, ConnectionState,
};
pub use pool_state_manager::{
    PoolStateManager, DataSourceType, PoolFilterConfig, PoolStateStats, 
    PoolStatus, RegistrySizes, CachedPoolState, WorkerResult, PoolDiscovery,
};
