use thiserror::Error;

#[derive(Debug, Error)]
pub enum SummarizeError {
    #[error("api error (status {status}): {body}")]
    Api { status: u16, body: String },

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to parse LLM response: {0}")]
    ParseResponse(String),

    #[error("store error: {0}")]
    Store(#[from] chizu_core::ChizuError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SummarizeError>;
