use crate::types::MarketDataIngestorConfig;
use anyhow::Result;
use aptos_indexer_processor_sdk::aptos_indexer_transaction_stream::TransactionStreamConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexerProcessorConfig {
    pub transaction_stream_config: TransactionStreamConfig,
    pub market_data_config: MarketDataIngestorConfig,
}

impl IndexerProcessorConfig {
    pub fn load(path: PathBuf) -> Result<Self> {
        let mut file = std::fs::File::open(&path)?;
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut file, &mut contents)?;
        serde_yaml::from_str(&contents).map_err(anyhow::Error::from)
    }
}
