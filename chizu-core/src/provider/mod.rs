use std::time::Duration;

use async_trait::async_trait;

pub mod bedrock;
pub mod openai;

pub use bedrock::BedrockProvider;
pub use openai::OpenAiProvider;

/// Errors that can occur when calling a provider.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error: {status}: {message}")]
    Api { status: u16, message: String },
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("timeout")]
    Timeout,
    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for ProviderError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            ProviderError::Timeout
        } else {
            ProviderError::Http(err.to_string())
        }
    }
}

/// Abstraction over LLM/embedding providers.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Generate a text completion for the given prompt.
    async fn complete(
        &self,
        prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<String, ProviderError>;

    /// Generate embeddings for a batch of texts.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError>;
}

/// Retry a closure with exponential backoff.
pub async fn with_retry<T, F, Fut>(
    attempts: u32,
    base_delay: Duration,
    mut f: F,
) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ProviderError>>,
{
    let mut last_err = None;
    for attempt in 0..attempts {
        match f().await {
            Ok(val) => return Ok(val),
            Err(err) => {
                let should_retry = match &err {
                    ProviderError::Timeout | ProviderError::Http(_) => true,
                    ProviderError::Api { status, .. } => *status >= 500,
                    _ => false,
                };
                if !should_retry || attempt == attempts - 1 {
                    return Err(err);
                }
                last_err = Some(err);
                let delay = base_delay * 2_u32.pow(attempt);
                tokio::time::sleep(delay).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| ProviderError::Other("retry exhausted".into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn retry_succeeds_eventually() {
        let calls = AtomicUsize::new(0);
        let result = with_retry(3, Duration::from_millis(1), || async {
            let call = calls.fetch_add(1, Ordering::Relaxed) + 1;
            if call < 3 {
                Err(ProviderError::Timeout)
            } else {
                Ok("success")
            }
        })
        .await;
        assert_eq!(result.unwrap(), "success");
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn retry_gives_up_on_non_retryable_error() {
        let calls = AtomicUsize::new(0);
        let result: Result<&str, _> = with_retry(3, Duration::from_millis(1), || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Err(ProviderError::Api {
                status: 400,
                message: "bad request".into(),
            })
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
