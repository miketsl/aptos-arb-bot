use config_lib::FilterConfig;
use crate::types::MarketUpdate;
use common::types::TokenPair;

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
            PoolFilter::TokenPairs(pairs) => pairs.iter().any(|(a, b)|
                (a == &pair.token0 && b == &pair.token1) || (a == &pair.token1 && b == &pair.token0)
            ),
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

    /// Apply the filter to a batch of market updates, dropping non-matching pools.
    pub fn filter(&self, updates: Vec<MarketUpdate>) -> Vec<MarketUpdate> {
        updates
            .into_iter()
            .filter(|u| self.filter.matches(&u.token_pair))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::TokenPair;
    use config_lib::FilterConfig;

    fn mk_update(pair: (&str, &str)) -> MarketUpdate {
        MarketUpdate {
            pool_address: "p".to_string(),
            dex_name: "d".to_string(),
            token_pair: TokenPair { token0: pair.0.to_string(), token1: pair.1.to_string() },
            sqrt_price: 0,
            liquidity: 0,
            tick: 0,
            fee_bps: 0,
            tick_map: Default::default(),
        }
    }

    #[test]
    fn test_filter_step_token_pairs() {
        let cfg = FilterConfig::TokenPairs { token_pairs: vec![("A".into(), "B".into())] };
        let step = FilterStep::new(&cfg);
        let updates = vec![mk_update(("A","B")), mk_update(("B","C"))];
        let out = step.filter(updates);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].token_pair, TokenPair { token0: "A".into(), token1: "B".into() });
    }

    #[test]
    fn test_filter_step_token_all() {
        let cfg = FilterConfig::All;
        let step = FilterStep::new(&cfg);
        let updates = vec![mk_update(("X","Y")), mk_update(("Y","Z"))];
        let out = step.filter(updates.clone());
        // All updates should pass filter
        assert_eq!(out.len(), updates.len());
        assert_eq!(out[0].token_pair, updates[0].token_pair);
        assert_eq!(out[1].token_pair, updates[1].token_pair);
    }

    #[test]
    fn test_filter_step_token_single() {
        let cfg = FilterConfig::Token { token: "X".into() };
        let step = FilterStep::new(&cfg);
        let updates = vec![mk_update(("X","Y")), mk_update(("A","B"))];
        let out = step.filter(updates);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].token_pair, TokenPair { token0: "X".into(), token1: "Y".into() });
    }
}