use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStreamConfig;
use config_lib::{MarketDataConfig, YamlTransactionStreamConfig};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IndexerProcessorConfig {
    pub transaction_stream_config: TransactionStreamConfig,
    pub market_data_config: MarketDataConfig,
}

impl IndexerProcessorConfig {
    pub fn new(
        yaml_config: YamlTransactionStreamConfig,
        market_data_config: MarketDataConfig,
    ) -> Self {
        let transaction_stream_config =
            serde_json::from_value(serde_json::to_value(yaml_config).unwrap())
                .expect("Failed to deserialize transaction stream config");

        Self {
            transaction_stream_config,
            market_data_config,
        }
    }
}
