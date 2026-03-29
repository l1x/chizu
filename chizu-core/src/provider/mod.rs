use std::time::Duration;

pub mod openai;

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
pub trait Provider: Send + Sync {
    /// Generate a text completion for the given prompt.
    fn complete(&self, prompt: &str) -> Result<String, ProviderError>;

    /// Generate embeddings for a batch of texts.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError>;
}

/// Retry a closure with exponential backoff.
pub fn with_retry<T, F>(
    attempts: u32,
    base_delay: Duration,
    mut f: F,
) -> Result<T, ProviderError>
where
    F: FnMut() -> Result<T, ProviderError>,
{
    let mut last_err = None;
    for attempt in 0..attempts {
        match f() {
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
                std::thread::sleep(delay);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| ProviderError::Other("retry exhausted".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_succeeds_eventually() {
        let mut calls = 0;
        let result = with_retry(3, Duration::from_millis(1), || {
            calls += 1;
            if calls < 3 {
                Err(ProviderError::Timeout)
            } else {
                Ok("success")
            }
        });
        assert_eq!(result.unwrap(), "success");
        assert_eq!(calls, 3);
    }

    #[test]
    fn retry_gives_up_on_non_retryable_error() {
        let mut calls = 0;
        let result: Result<&str, _> = with_retry(3, Duration::from_millis(1), || {
            calls += 1;
            Err(ProviderError::Api {
                status: 400,
                message: "bad request".into(),
            })
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}
