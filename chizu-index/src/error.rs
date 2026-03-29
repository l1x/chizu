use std::path::PathBuf;

/// Errors that can occur during indexing.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("store error: {0}")]
    Store(#[from] chizu_core::StoreError),

    #[error("config error: {0}")]
    Config(#[from] chizu_core::ConfigError),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("walk error: {0}")]
    Walk(String),

    #[error("globset error: {0}")]
    Globset(#[from] globset::Error),

    #[error("invalid manifest at {path}: {message}")]
    InvalidManifest { path: PathBuf, message: String },

    #[error("{0}")]
    Other(String),
}

/// Result type for indexing operations.
pub type Result<T> = std::result::Result<T, IndexError>;
