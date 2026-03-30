#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("store error: {0}")]
    Store(#[from] chizu_core::StoreError),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, QueryError>;
