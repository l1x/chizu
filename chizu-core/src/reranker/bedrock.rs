use async_trait::async_trait;
use aws_sdk_bedrockagentruntime as bedrockagentruntime;
use bedrockagentruntime::types::{
    BedrockRerankingConfiguration, BedrockRerankingModelConfiguration, RerankDocument,
    RerankDocumentType, RerankQuery, RerankQueryContentType, RerankSource, RerankSourceType,
    RerankTextDocument, RerankingConfiguration, RerankingConfigurationType,
};

use crate::aws::new_bedrock_agent_runtime_client;
use crate::config::{ProviderConfig, RerankerConfig};

use super::{RerankDocument as InputDocument, RerankScore, Reranker, RerankerError};

/// Amazon Bedrock reranker backed by the Agents runtime `Rerank` API.
pub struct BedrockReranker {
    client: bedrockagentruntime::Client,
    model_arn: String,
}

impl BedrockReranker {
    pub async fn new(
        provider_config: &ProviderConfig,
        reranker_config: &RerankerConfig,
    ) -> Result<Self, RerankerError> {
        let model_arn = reranker_config
            .model
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| {
                RerankerError::Unavailable(
                    "reranker.model is required for aws_bedrock rerankers".into(),
                )
            })?;

        Ok(Self {
            client: new_bedrock_agent_runtime_client(provider_config).await,
            model_arn,
        })
    }

    fn map_sdk_error(err: impl std::fmt::Display) -> RerankerError {
        let message = err.to_string();
        if message.to_ascii_lowercase().contains("timeout") {
            RerankerError::Timeout
        } else {
            RerankerError::Unavailable(format!("bedrock rerank error: {message}"))
        }
    }
}

#[async_trait]
impl Reranker for BedrockReranker {
    async fn rerank(
        &self,
        query: &str,
        documents: &[InputDocument],
    ) -> Result<Vec<RerankScore>, RerankerError> {
        if documents.is_empty() {
            return Ok(vec![]);
        }

        let query = RerankQuery::builder()
            .r#type(RerankQueryContentType::Text)
            .text_query(RerankTextDocument::builder().text(query).build())
            .build()
            .map_err(|e| RerankerError::Json(e.to_string()))?;

        let mut request = self.client.rerank().queries(query);
        for document in documents {
            let document = RerankDocument::builder()
                .r#type(RerankDocumentType::Text)
                .text_document(RerankTextDocument::builder().text(&document.text).build())
                .build()
                .map_err(|e| RerankerError::Json(e.to_string()))?;

            let source = RerankSource::builder()
                .r#type(RerankSourceType::Inline)
                .inline_document_source(document)
                .build()
                .map_err(|e| RerankerError::Json(e.to_string()))?;

            request = request.sources(source);
        }

        let reranking_configuration = RerankingConfiguration::builder()
            .r#type(RerankingConfigurationType::BedrockRerankingModel)
            .bedrock_reranking_configuration(
                BedrockRerankingConfiguration::builder()
                    .number_of_results(documents.len() as i32)
                    .model_configuration(
                        BedrockRerankingModelConfiguration::builder()
                            .model_arn(&self.model_arn)
                            .build()
                            .map_err(|e| RerankerError::Json(e.to_string()))?,
                    )
                    .build(),
            )
            .build()
            .map_err(|e| RerankerError::Json(e.to_string()))?;

        let response = request
            .reranking_configuration(reranking_configuration)
            .send()
            .await
            .map_err(Self::map_sdk_error)?;

        let mut scores = Vec::with_capacity(response.results().len());
        for result in response.results() {
            scores.push(RerankScore {
                index: result.index() as usize,
                score: result.relevance_score() as f64,
            });
        }

        Ok(scores)
    }
}
