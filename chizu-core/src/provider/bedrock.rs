use std::time::Duration;

use async_trait::async_trait;
use aws_sdk_bedrockruntime as bedrockruntime;
use bedrockruntime::primitives::Blob;
use bedrockruntime::types::{ContentBlock, ConversationRole, InferenceConfiguration, Message};

use crate::aws::new_bedrock_runtime_client;
use crate::config::ProviderConfig;
use crate::provider::{Provider, ProviderError, with_retry};

const RETRY_BASE_DELAY_MS: u64 = 500;

/// Amazon Bedrock-backed provider for summaries and embeddings.
pub struct BedrockProvider {
    client: bedrockruntime::Client,
    completion_model: String,
    embedding_model: String,
    embedding_dimensions: Option<u32>,
    retry_attempts: u32,
}

impl BedrockProvider {
    pub async fn new(
        config: &ProviderConfig,
        completion_model: String,
        embedding_model: String,
        embedding_dimensions: Option<u32>,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            client: new_bedrock_runtime_client(config).await,
            completion_model,
            embedding_model,
            embedding_dimensions,
            retry_attempts: config.retry_attempts.max(1),
        })
    }

    fn map_sdk_error(err: impl std::fmt::Display) -> ProviderError {
        let message = err.to_string();
        if message.to_ascii_lowercase().contains("timeout") {
            ProviderError::Timeout
        } else {
            ProviderError::Other(format!("bedrock runtime error: {message}"))
        }
    }

    fn collect_text_from_message(message: &Message) -> Result<String, ProviderError> {
        let mut output = String::new();
        for block in message.content() {
            if let ContentBlock::Text(text) = block {
                output.push_str(text);
            }
        }

        let trimmed = output.trim();
        if trimmed.is_empty() {
            return Err(ProviderError::Other(
                "bedrock response did not contain text output".into(),
            ));
        }

        Ok(trimmed.to_string())
    }

    async fn embed_one(&self, text: &str) -> Result<Vec<f32>, ProviderError> {
        with_retry(
            self.retry_attempts,
            Duration::from_millis(RETRY_BASE_DELAY_MS),
            || async {
                let mut body = serde_json::json!({
                    "inputText": text,
                    "normalize": true,
                });
                if let Some(dimensions) = self.embedding_dimensions {
                    body["dimensions"] = serde_json::json!(dimensions);
                }

                let payload = serde_json::to_vec(&body)?;
                let response = self
                    .client
                    .invoke_model()
                    .model_id(&self.embedding_model)
                    .content_type("application/json")
                    .accept("application/json")
                    .body(Blob::new(payload))
                    .send()
                    .await
                    .map_err(Self::map_sdk_error)?;

                let json: serde_json::Value = serde_json::from_slice(response.body().as_ref())?;
                let embedding = json
                    .get("embedding")
                    .or_else(|| json.get("embeddingsByType").and_then(|v| v.get("float")))
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| ProviderError::Other("missing embedding vector".into()))?;

                embedding
                    .iter()
                    .map(|value| {
                        value
                            .as_f64()
                            .map(|v| v as f32)
                            .ok_or_else(|| ProviderError::Other("invalid embedding value".into()))
                    })
                    .collect()
            },
        )
        .await
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    async fn complete(
        &self,
        prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<String, ProviderError> {
        with_retry(
            self.retry_attempts,
            Duration::from_millis(RETRY_BASE_DELAY_MS),
            || async {
                let user_message = Message::builder()
                    .role(ConversationRole::User)
                    .content(ContentBlock::Text(prompt.to_string()))
                    .build()
                    .map_err(|e| ProviderError::Other(format!("invalid Bedrock message: {e}")))?;

                let mut inference = InferenceConfiguration::builder().temperature(0.2);
                if let Some(tokens) = max_tokens {
                    inference = inference.max_tokens(tokens as i32);
                }

                let response = self
                    .client
                    .converse()
                    .model_id(&self.completion_model)
                    .messages(user_message)
                    .inference_config(inference.build())
                    .send()
                    .await
                    .map_err(Self::map_sdk_error)?;

                let message = response
                    .output()
                    .and_then(|output| output.as_message().ok())
                    .ok_or_else(|| {
                        ProviderError::Other("bedrock response did not contain a message".into())
                    })?;

                Self::collect_text_from_message(message)
            },
        )
        .await
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.embed_one(text).await?);
        }
        Ok(embeddings)
    }
}
