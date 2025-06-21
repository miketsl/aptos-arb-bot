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
