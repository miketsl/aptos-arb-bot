use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStreamConfig;
use config_lib::{IngestorConfig, YamlTransactionStreamConfig};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IndexerProcessorConfig {
    pub transaction_stream_config: TransactionStreamConfig,
    pub ingestor_config: IngestorConfig,
}

impl IndexerProcessorConfig {
    pub fn from_enhanced_config(
        yaml_config: YamlTransactionStreamConfig,
        ingestor_config: IngestorConfig,
    ) -> Self {
        let transaction_stream_config =
            serde_json::from_value(serde_json::to_value(yaml_config).unwrap())
                .expect("Failed to deserialize transaction stream config");

        Self {
            transaction_stream_config,
            ingestor_config,
        }
    }
}
