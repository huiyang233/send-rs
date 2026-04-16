use thiserror::Error;

pub type Result<T> = std::result::Result<T, SendRsError>;

#[derive(Debug, Error)]
pub enum SendRsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("peer not paired: {0}")]
    PeerNotPaired(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("internal error: {0}")]
    Internal(String),
}
