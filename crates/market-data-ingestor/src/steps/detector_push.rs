use anyhow::Result;
use common::types::DetectorMessage;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

/// Queue operation metrics for a single send operation
#[derive(Debug, Clone)]
pub struct QueueOperationMetrics {
    pub operation_time_ms: f64,
    pub queue_depth_before: usize,
    pub was_blocked: bool,
    pub channel_capacity: usize,
}

impl QueueOperationMetrics {
    pub fn new(operation_time_ms: f64, queue_depth_before: usize, was_blocked: bool, channel_capacity: usize) -> Self {
        Self {
            operation_time_ms,
            queue_depth_before,
            was_blocked,
            channel_capacity,
        }
    }
}

/// Step that pushes detector messages (BlockStart/MarketUpdate/BlockEnd) with queue monitoring.
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

    /// Send a detector message with queue monitoring and timing
    pub async fn push_with_metrics(&self, msg: DetectorMessage) -> Result<QueueOperationMetrics> {
        let start_time = Instant::now();
        
        // Get current channel state for monitoring
        let channel_capacity = self.sender.capacity();
        let queue_depth_before = channel_capacity.saturating_sub(self.sender.capacity());
        
        debug!(?msg, queue_depth = queue_depth_before, "Pushing detector message with metrics");

        // Track if the operation blocks (takes longer than expected)
        // Operations taking >1ms likely indicate backpressure
        let send_result = self.sender.send(msg).await;
        let operation_time = start_time.elapsed();
        let operation_time_ms = operation_time.as_micros() as f64 / 1000.0;
        
        // Detect blocking based on operation time and queue depth
        let was_blocked = operation_time_ms > 1.0 || queue_depth_before as f64 > channel_capacity as f64 * 0.8;
        
        if was_blocked {
            warn!(
                operation_time_ms = operation_time_ms,
                queue_depth = queue_depth_before,
                channel_capacity = channel_capacity,
                "Detected backpressure in detector message queue"
            );
        }

        if let Err(e) = send_result {
            error!(error = %e, operation_time_ms = operation_time_ms, "Failed to send detector message");
            return Err(anyhow::anyhow!("Channel send failed: {}", e));
        }

        Ok(QueueOperationMetrics::new(
            operation_time_ms,
            queue_depth_before,
            was_blocked,
            channel_capacity,
        ))
    }
}
