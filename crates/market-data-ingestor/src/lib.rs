pub mod ingestor_config;
pub mod processor;
pub mod steps;
pub mod types;
pub mod data_source;

pub use ingestor_config::IndexerProcessorConfig;
pub use processor::MarketDataIngestorProcessor;

// Export the data source abstractions
pub use data_source::{
    DataSource, DataSourceError, RawEvent, TimestampedEvent, EventMetadata,
    GrpcSource, FileSource, RecordedBatch,
};
