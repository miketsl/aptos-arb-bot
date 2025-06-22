use crate::graph::AssetId;

/// Errors encountered during detector operation.
#[derive(Debug)]
pub enum DetectorError {
    /// A disconnected component was encountered in the graph.
    DisconnectedComponent(AssetId),
    /// The graph is corrupted and needs rebuilding.
    GraphCorruption,
    /// An edge update failed during transformation or insertion.
    EdgeUpdateFailed(String),

    /// A strategy execution failed. Contains the strategy name and error message.
    StrategyFailed(String, String),
    /// An invalid opportunity was detected.
    InvalidOpportunity(String),

    /// The channel to the downstream consumer is closed.
    ChannelClosed,
    /// A market update transformation failed.
    TransformError(String),
}
