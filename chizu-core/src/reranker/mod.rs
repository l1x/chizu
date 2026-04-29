pub mod bedrock;
pub mod http;

use async_trait::async_trait;

pub use bedrock::BedrockReranker;
pub use http::HttpReranker;

/// A document to be reranked.
pub struct RerankDocument {
    pub text: String,
}

/// Reranking score for a document.
#[derive(Debug)]
pub struct RerankScore {
    pub index: usize,
    pub score: f64,
}

/// Errors from reranking operations.
#[derive(Debug, thiserror::Error)]
pub enum RerankerError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("JSON error: {0}")]
    Json(String),
    #[error("reranker timeout")]
    Timeout,
    #[error("reranker unavailable: {0}")]
    Unavailable(String),
}

impl From<reqwest::Error> for RerankerError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            RerankerError::Timeout
        } else {
            RerankerError::Http(err.to_string())
        }
    }
}

/// Trait for second-stage rerankers.
///
/// A reranker takes a query and a list of documents, and returns relevance
/// scores that determine the final ordering. The scores from a reranker
/// replace (not blend with) the first-stage scores.
#[async_trait]
pub trait Reranker: Send + Sync {
    async fn rerank(
        &self,
        query: &str,
        documents: &[RerankDocument],
    ) -> Result<Vec<RerankScore>, RerankerError>;
}
