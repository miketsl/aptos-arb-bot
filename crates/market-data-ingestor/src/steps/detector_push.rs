use super::super::types::MarketUpdate;
use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Step that pushes market updates to the detector via a channel
pub struct DetectorPushStep {
    sender: mpsc::Sender<MarketUpdate>,
}

impl DetectorPushStep {
    pub fn new(sender: mpsc::Sender<MarketUpdate>) -> Self {
        Self { sender }
    }

    pub async fn push_updates(&self, updates: Vec<MarketUpdate>) -> Result<()> {
        for update in updates {
            debug!(
                pool = update.pool_address,
                dex = update.dex_name,
                "Pushing market update to detector"
            );

            if let Err(e) = self.sender.send(update).await {
                error!(error = %e, "Failed to send update to detector");
                return Err(anyhow::anyhow!("Channel send failed: {}", e));
            }
        }

        Ok(())
    }
}
