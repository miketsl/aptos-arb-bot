pub mod ingestor_config;
pub mod processor;
pub mod steps;
pub mod types;
pub mod data_source;
pub mod pool_state_manager;

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
