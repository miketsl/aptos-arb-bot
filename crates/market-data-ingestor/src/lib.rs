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
    ConnectionState,
    ConnectionStats,
    DataSource,
    DataSourceError,
    EventMetadata,
    FileSource,
    FileSource as FileDataSource, // Alias for test compatibility
    GrpcSource,
    PoolInitialization,
    RawEvent,
    ReconnectionConfig,
    RecordedBatch,
    RecordedPoolState,
    TimestampedEvent,
};
pub use pool_state_manager::{
    CachedPoolState, DataSourceType, PoolDiscovery, PoolFilterConfig, PoolStateManager,
    PoolStateStats, PoolStatus, RegistrySizes, WorkerResult,
};
pub use recording_config::MonitoringSettings;
