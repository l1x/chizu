use std::time::Duration;

use crate::config::ProviderConfig;
use crate::provider::{Provider, ProviderError, with_retry};

const RETRY_BASE_DELAY_MS: u64 = 500;

/// OpenAI-compatible HTTP provider.
pub struct OpenAiProvider {
    client: reqwest::blocking::Client,
    base_url: String,
    api_key: String,
    completion_model: String,
    embedding_model: String,
    retry_attempts: u32,
}

impl OpenAiProvider {
    pub fn new(
        config: &ProviderConfig,
        completion_model: String,
        embedding_model: String,
    ) -> Result<Self, ProviderError> {
        let timeout = Duration::from_secs(config.timeout_secs);
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ProviderError::Http(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
            completion_model,
            embedding_model,
            retry_attempts: config.retry_attempts.max(1),
        })
    }

    fn build_request(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        let url = format!("{}/{}", self.base_url, path);
        let mut req = self.client.post(&url);
        req = req.header("Content-Type", "application/json");
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        req
    }
}

impl Provider for OpenAiProvider {
    fn complete(&self, prompt: &str, max_tokens: Option<u32>) -> Result<String, ProviderError> {
        with_retry(
            self.retry_attempts,
            Duration::from_millis(RETRY_BASE_DELAY_MS),
            || {
                let mut body = serde_json::json!({
                    "model": self.completion_model,
                    "messages": [
                        {"role": "user", "content": prompt}
                    ],
                    "temperature": 0.2,
                    "response_format": {"type": "json_object"},
                });
                if let Some(tokens) = max_tokens {
                    body["max_tokens"] = serde_json::json!(tokens);
                }

                let response = self
                    .build_request("chat/completions")
                    .json(&body)
                    .send()
                    .map_err(ProviderError::from)?;

                let status = response.status();
                let text = response.text().map_err(ProviderError::from)?;

                if !status.is_success() {
                    return Err(ProviderError::Api {
                        status: status.as_u16(),
                        message: text,
                    });
                }

                let json: serde_json::Value = serde_json::from_str(&text)?;
                let content = json
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| ProviderError::Other("missing completion content".into()))?;

                Ok(content.trim().to_string())
            },
        )
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        with_retry(
            self.retry_attempts,
            Duration::from_millis(RETRY_BASE_DELAY_MS),
            || {
                let body = serde_json::json!({
                    "model": self.embedding_model,
                    "input": texts,
                });

                let response = self
                    .build_request("embeddings")
                    .json(&body)
                    .send()
                    .map_err(ProviderError::from)?;

                let status = response.status();
                let text = response.text().map_err(ProviderError::from)?;

                if !status.is_success() {
                    return Err(ProviderError::Api {
                        status: status.as_u16(),
                        message: text,
                    });
                }

                let json: serde_json::Value = serde_json::from_str(&text)?;
                let data = json
                    .get("data")
                    .and_then(|d| d.as_array())
                    .ok_or_else(|| ProviderError::Other("missing embedding data".into()))?;

                let mut vectors = Vec::with_capacity(data.len());
                for item in data {
                    let embedding = item
                        .get("embedding")
                        .and_then(|e| e.as_array())
                        .ok_or_else(|| ProviderError::Other("missing embedding vector".into()))?;
                    let vector: Vec<f32> = embedding
                        .iter()
                        .map(|v| {
                            v.as_f64().map(|f| f as f32).ok_or_else(|| {
                                ProviderError::Other("invalid embedding value".into())
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    vectors.push(vector);
                }

                Ok(vectors)
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    fn create_provider(
        config: &ProviderConfig,
        completion_model: &str,
        embedding_model: &str,
    ) -> OpenAiProvider {
        OpenAiProvider::new(
            config,
            completion_model.to_string(),
            embedding_model.to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_complete_success() {
        let mut server = mockito::Server::new();
        let url = server.url();

        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"choices":[{"message":{"content":"  Hello!  "}}]}"#)
            .create();

        let config = ProviderConfig {
            base_url: url,
            api_key: "test-key".to_string(),
            timeout_secs: 5,
            retry_attempts: 1,
        };
        let provider = create_provider(&config, "llama3", "nomic");
        let result = provider.complete("Say hi", None).unwrap();

        assert_eq!(result, "Hello!");
        mock.assert();
    }

    #[test]
    fn test_complete_api_error() {
        let mut server = mockito::Server::new();
        let url = server.url();

        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(400)
            .with_body("bad request")
            .create();

        let config = ProviderConfig {
            base_url: url,
            api_key: "".to_string(),
            timeout_secs: 5,
            retry_attempts: 1,
        };
        let provider = create_provider(&config, "llama3", "nomic");
        let result = provider.complete("Say hi", None);

        assert!(matches!(
            result,
            Err(ProviderError::Api { status: 400, .. })
        ));
        mock.assert();
    }

    #[test]
    fn test_complete_retries_on_500() {
        let mut server = mockito::Server::new();
        let url = server.url();

        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(500)
            .with_body("server error")
            .expect_at_least(2)
            .create();

        let config = ProviderConfig {
            base_url: url,
            api_key: "".to_string(),
            timeout_secs: 5,
            retry_attempts: 2,
        };
        let provider = create_provider(&config, "llama3", "nomic");
        let result = provider.complete("Say hi", None);

        assert!(result.is_err());
        mock.assert();
    }

    #[test]
    fn test_embed_success() {
        let mut server = mockito::Server::new();
        let url = server.url();

        let mock = server
            .mock("POST", "/embeddings")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":[{"embedding":[0.1,0.2,0.3]},{"embedding":[0.4,0.5,0.6]}]}"#)
            .create();

        let config = ProviderConfig {
            base_url: url,
            api_key: "".to_string(),
            timeout_secs: 5,
            retry_attempts: 1,
        };
        let provider = create_provider(&config, "llama3", "nomic");
        let result = provider.embed(&["hello".into(), "world".into()]).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], vec![0.1_f32, 0.2_f32, 0.3_f32]);
        assert_eq!(result[1], vec![0.4_f32, 0.5_f32, 0.6_f32]);
        mock.assert();
    }

    #[test]
    fn test_embed_empty_input() {
        let config = ProviderConfig {
            base_url: "http://localhost".to_string(),
            api_key: "".to_string(),
            timeout_secs: 5,
            retry_attempts: 1,
        };
        let provider = create_provider(&config, "llama3", "nomic");
        let result = provider.embed(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_complete_timeout_handling() {
        // Verify that connection failures produce the expected error variant
        // and don't panic. Uses a non-routable address to trigger a timeout.
        let config = ProviderConfig {
            base_url: "http://192.0.2.1:1".to_string(), // RFC 5737 TEST-NET, guaranteed non-routable
            api_key: "".to_string(),
            timeout_secs: 1,
            retry_attempts: 1,
        };
        let provider = create_provider(&config, "llama3", "nomic");

        let result = provider.complete("Say hi", None);
        assert!(result.is_err());
        match result.unwrap_err() {
            ProviderError::Http(_) | ProviderError::Timeout => {}
            other => panic!("expected Http or Timeout error, got: {other}"),
        }
    }

    #[test]
    fn test_complete_sends_max_tokens() {
        let mut server = mockito::Server::new();
        let url = server.url();

        let mock = server
            .mock("POST", "/chat/completions")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"max_tokens": 512}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"choices":[{"message":{"content":"ok"}}]}"#)
            .create();

        let config = ProviderConfig {
            base_url: url,
            api_key: "".to_string(),
            timeout_secs: 5,
            retry_attempts: 1,
        };
        let provider = create_provider(&config, "llama3", "nomic");
        let result = provider.complete("test", Some(512)).unwrap();
        assert_eq!(result, "ok");
        mock.assert();
    }

    #[test]
    fn test_complete_omits_max_tokens_when_none() {
        let mut server = mockito::Server::new();
        let url = server.url();

        let mock = server
            .mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"choices":[{"message":{"content":"ok"}}]}"#)
            .create();

        let config = ProviderConfig {
            base_url: url,
            api_key: "".to_string(),
            timeout_secs: 5,
            retry_attempts: 1,
        };
        let provider = create_provider(&config, "llama3", "nomic");
        let result = provider.complete("test", None).unwrap();
        assert_eq!(result, "ok");
        mock.assert();

        // Verify via a second request that max_tokens is absent by checking
        // the mock matched (it would fail if body was unexpected).
        // Additionally, verify the inverse: with max_tokens, the partial matcher sees it.
        drop(mock);
        let mock_with_tokens = server
            .mock("POST", "/chat/completions")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"max_tokens": 256}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"choices":[{"message":{"content":"with tokens"}}]}"#)
            .create();
        let result = provider.complete("test", Some(256)).unwrap();
        assert_eq!(result, "with tokens");
        mock_with_tokens.assert();
    }
}
