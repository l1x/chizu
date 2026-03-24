use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraknoError {
    #[cfg(feature = "sqlite_usearch")]
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[cfg(feature = "grafeo")]
    #[error("grafeo error: {0}")]
    Grafeo(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, GraknoError>;
