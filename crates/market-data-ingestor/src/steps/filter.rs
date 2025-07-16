use common::types::MarketUpdate;
use common::types::TokenPair;
use config_lib::{FilterConfig, IngestorFilterConfig};

/// Filter criteria for selecting which CLMM pools to ingest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolFilter {
    /// All pools.
    All,
    /// All pools containing this token symbol.
    Token(String),
    /// Specific token pairs (unordered).
    TokenPairs(Vec<(String, String)>),
}

impl PoolFilter {
    /// Returns true if the given token pair matches this filter.
    pub fn matches(&self, pair: &TokenPair) -> bool {
        match self {
            PoolFilter::All => true,
            PoolFilter::Token(tok) => &pair.token0 == tok || &pair.token1 == tok,
            PoolFilter::TokenPairs(pairs) => pairs.iter().any(|(a, b)| {
                (a == &pair.token0 && b == &pair.token1) || (a == &pair.token1 && b == &pair.token0)
            }),
        }
    }
}

/// A processing step that filters `MarketUpdate`s based on token or token-pair criteria.
/// A processing step that filters `MarketUpdate`s based on token or token-pair criteria.
pub struct FilterStep {
    filter: PoolFilter,
}

impl FilterStep {
    /// Create a new `FilterStep` from the shared configuration filter.
    pub fn new(cfg: &FilterConfig) -> Self {
        let filter = match cfg {
            FilterConfig::All => PoolFilter::All,
            FilterConfig::Token { token } => PoolFilter::Token(token.clone()),
            FilterConfig::TokenPairs { token_pairs } => PoolFilter::TokenPairs(token_pairs.clone()),
        };
        FilterStep { filter }
    }

    /// Create a new `FilterStep` from the enhanced ingestor filter configuration.
    pub fn from_ingestor_config(cfg: &IngestorFilterConfig) -> Self {
        let filter = if !cfg.enabled {
            PoolFilter::All
        } else if let Some(ref pairs) = cfg.token_pairs {
            PoolFilter::TokenPairs(pairs.clone())
        } else if let Some(ref whitelist) = cfg.token_whitelist {
            if whitelist.len() == 1 {
                PoolFilter::Token(whitelist[0].clone())
            } else {
                // For multiple tokens, create all possible pairs
                let mut pairs = Vec::new();
                for i in 0..whitelist.len() {
                    for j in i + 1..whitelist.len() {
                        pairs.push((whitelist[i].clone(), whitelist[j].clone()));
                    }
                }
                PoolFilter::TokenPairs(pairs)
            }
        } else {
            PoolFilter::All
        };
        FilterStep { filter }
    }

    /// Retain only updates matching the configured token/pair filter.
    pub fn apply(&self, updates: &mut Vec<MarketUpdate>) {
        updates.retain(|u| match u {
            MarketUpdate::Clmm(m) => self.filter.matches(&m.token_pair),
            MarketUpdate::ConstantProduct(m) => self.filter.matches(&m.token_pair),
            MarketUpdate::StableSwap(m) => self.filter.matches(&m.token_pair),
            MarketUpdate::WeightedPool(m) => self.filter.matches(&m.token_pair),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{ClmmMarketUpdate, TokenPair};
    use config_lib::FilterConfig;

    fn mk_update(pair: (&str, &str)) -> MarketUpdate {
        MarketUpdate::Clmm(ClmmMarketUpdate {
            pool_address: "p".to_string(),
            dex_name: "d".to_string(),
            token_pair: TokenPair {
                token0: pair.0.to_string(),
                token1: pair.1.to_string(),
            },
            sqrt_price: 0,
            liquidity: 0,
            tick: 0,
            fee_bps: 0,
            tick_map: Default::default(),
        })
    }

    #[test]
    fn test_filter_step_token_pairs() {
        let cfg = FilterConfig::TokenPairs {
            token_pairs: vec![("A".into(), "B".into())],
        };
        let step = FilterStep::new(&cfg);
        let mut updates = vec![mk_update(("A", "B")), mk_update(("B", "C"))];
        step.apply(&mut updates);
        assert_eq!(updates.len(), 1);
        let token_pair = match &updates[0] {
            MarketUpdate::Clmm(m) => &m.token_pair,
            _ => panic!("Expected ClmmMarketUpdate"),
        };
        assert_eq!(
            token_pair,
            &TokenPair {
                token0: "A".into(),
                token1: "B".into()
            }
        );
    }

    #[test]
    fn test_filter_step_token_all() {
        let cfg = FilterConfig::All;
        let step = FilterStep::new(&cfg);
        let mut updates = vec![mk_update(("X", "Y")), mk_update(("Y", "Z"))];
        let orig = updates.clone();
        step.apply(&mut updates);
        // All updates should pass filter
        assert_eq!(updates.len(), orig.len());
        // Cannot compare enums directly with PartialEq unless derived.
        // Instead, we check the length, which is sufficient for this test.
        assert_eq!(updates.len(), orig.len());
    }

    #[test]
    fn test_filter_step_token_single() {
        let cfg = FilterConfig::Token { token: "X".into() };
        let step = FilterStep::new(&cfg);
        let mut updates = vec![mk_update(("X", "Y")), mk_update(("A", "B"))];
        step.apply(&mut updates);
        assert_eq!(updates.len(), 1);
        let token_pair = match &updates[0] {
            MarketUpdate::Clmm(m) => &m.token_pair,
            _ => panic!("Expected ClmmMarketUpdate"),
        };
        assert_eq!(
            token_pair,
            &TokenPair {
                token0: "X".into(),
                token1: "Y".into()
            }
        );
    }
}
