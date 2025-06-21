use anyhow::Result;
use common::types::DetectorMessage;
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Step that pushes detector messages (BlockStart/MarketUpdate/BlockEnd).
pub struct DetectorPushStep {
    sender: mpsc::Sender<DetectorMessage>,
}

impl DetectorPushStep {
    pub fn new(sender: mpsc::Sender<DetectorMessage>) -> Self {
        Self { sender }
    }

    /// Send a single detector message to the receiver.
    pub async fn push(&self, msg: DetectorMessage) -> Result<()> {
        debug!(?msg, "Pushing detector message");

        if let Err(e) = self.sender.send(msg).await {
            error!(error = %e, "Failed to send detector message");
            return Err(anyhow::anyhow!("Channel send failed: {}", e));
        }
        Ok(())
    }
}
