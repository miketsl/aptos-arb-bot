use common::types::MarketUpdate;
use common::types::TokenPair;
use config_lib::{FilterConfig, IngestorFilterConfig};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Categorizes why market updates were filtered out.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterReasons {
    pub filtered_by_token: u64,
    pub filtered_by_dex: u64,
    pub filtered_by_liquidity: u64,
    pub filtered_by_token_pairs: u64,
}

impl FilterReasons {
    pub fn new() -> Self {
        Self {
            filtered_by_token: 0,
            filtered_by_dex: 0,
            filtered_by_liquidity: 0,
            filtered_by_token_pairs: 0,
        }
    }

    pub fn add_token_filter(&mut self) {
        self.filtered_by_token += 1;
    }

    pub fn add_token_pairs_filter(&mut self) {
        self.filtered_by_token_pairs += 1;
    }

    pub fn add_dex_filter(&mut self) {
        self.filtered_by_dex += 1;
    }

    pub fn add_liquidity_filter(&mut self) {
        self.filtered_by_liquidity += 1;
    }
}

impl Default for FilterReasons {
    fn default() -> Self {
        Self::new()
    }
}

/// Detailed metrics for a single filter application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterMetrics {
    pub updates_received_total: u64,
    pub updates_after_filtering: u64,
    pub updates_filtered_out: u64,
    pub filter_pass_rate_percent: f64,
    pub filter_processing_time_ms: f64,
    pub filter_reasons: FilterReasons,
}

impl FilterMetrics {
    pub fn new(
        updates_received: u64,
        updates_after: u64,
        processing_time_ms: f64,
        filter_reasons: FilterReasons,
    ) -> Self {
        let updates_filtered_out = updates_received.saturating_sub(updates_after);
        let filter_pass_rate_percent = if updates_received > 0 {
            (updates_after as f64 / updates_received as f64) * 100.0
        } else {
            100.0
        };

        Self {
            updates_received_total: updates_received,
            updates_after_filtering: updates_after,
            updates_filtered_out,
            filter_pass_rate_percent,
            filter_processing_time_ms: processing_time_ms,
            filter_reasons,
        }
    }
}

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

    /// Apply filtering with detailed metrics collection.
    /// Returns metrics about the filtering operation including processing time and filter breakdown.
    pub fn apply_with_metrics(&self, updates: &mut Vec<MarketUpdate>) -> FilterMetrics {
        let start_time = Instant::now();
        let updates_received = updates.len() as u64;
        let mut filter_reasons = FilterReasons::new();

        // Track which updates are filtered and why
        updates.retain(|u| {
            let token_pair = match u {
                MarketUpdate::Clmm(m) => &m.token_pair,
                MarketUpdate::ConstantProduct(m) => &m.token_pair,
                MarketUpdate::StableSwap(m) => &m.token_pair,
                MarketUpdate::WeightedPool(m) => &m.token_pair,
            };

            let matches = self.filter.matches(token_pair);

            if !matches {
                // Categorize the filter reason based on filter type
                match &self.filter {
                    PoolFilter::All => {
                        // Should not happen since All matches everything
                    }
                    PoolFilter::Token(_) => {
                        filter_reasons.add_token_filter();
                    }
                    PoolFilter::TokenPairs(_) => {
                        filter_reasons.add_token_pairs_filter();
                    }
                }
            }

            matches
        });

        let processing_time_ms = start_time.elapsed().as_micros() as f64 / 1000.0;
        let updates_after = updates.len() as u64;

        FilterMetrics::new(
            updates_received,
            updates_after,
            processing_time_ms,
            filter_reasons,
        )
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
