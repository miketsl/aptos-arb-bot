pub mod ingestor_config;
pub mod processor;
pub mod steps;
pub mod types;
pub mod data_source;

pub use ingestor_config::IndexerProcessorConfig;
pub use processor::MarketDataIngestorProcessor;

// Export the new data source abstractions
pub use data_source::{
    DataSource, DataSourceError, RawEvent, TimestampedEvent, EventMetadata,
    EnhancedGrpcSource, EnhancedFileSource,
    // Keep legacy exports for backward compatibility
    LegacyDataSource, GrpcSource, FileSource, RecordedBatch,
};
