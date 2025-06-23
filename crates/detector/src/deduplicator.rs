use common::types::ArbitrageOpportunity;
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub struct OpportunityDeduplicator {
    seen: HashSet<[u8; 32]>,
    last_pruned: Instant,
    ttl: Duration,
}

impl OpportunityDeduplicator {
    pub fn new(ttl: Duration) -> Self {
        Self {
            seen: HashSet::new(),
            last_pruned: Instant::now(),
            ttl,
        }
    }

    /// Checks if an opportunity is a duplicate. If not, it's added to the set.
    pub fn is_duplicate(&mut self, opportunity: &ArbitrageOpportunity) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_pruned) > self.ttl {
            self.seen.clear();
            self.last_pruned = now;
        }

        let hash = opportunity.hash();
        if self.seen.contains(&hash) {
            true
        } else {
            self.seen.insert(hash);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::types::ArbitrageOpportunity;
    use rust_decimal_macros::dec;
    use std::{thread, time::Duration};
    use uuid::Uuid;

    fn make_opportunity() -> ArbitrageOpportunity {
        ArbitrageOpportunity {
            id: Uuid::new_v4(),
            strategy: "strat".to_string(),
            path: Vec::new(),
            expected_profit: dec!(1),
            input_amount: dec!(1),
            gas_estimate: 0,
            block_number: 0,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_deduplicator_seen_and_cleared() {
        let mut dedup = OpportunityDeduplicator::new(Duration::from_millis(50));
        let opp = make_opportunity();
        // First time: not duplicate
        assert!(!dedup.is_duplicate(&opp));
        // Immediately after: duplicate
        assert!(dedup.is_duplicate(&opp));
        // After TTL: cleared
        thread::sleep(Duration::from_millis(60));
        assert!(!dedup.is_duplicate(&opp));
    }
}
