use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("store error: {0}")]
    Store(#[from] chizu_core::ChizuError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("cargo metadata error: {0}")]
    CargoMetadata(#[from] cargo_metadata::Error),

    #[error("parse error: {0}")]
    Parse(String),
}
