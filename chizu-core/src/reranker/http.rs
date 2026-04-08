use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::RerankerConfig;

use super::{RerankDocument, RerankScore, Reranker, RerankerError};

/// HTTP-based reranker compatible with Jina, Cohere, and TEI `/rerank` APIs.
pub struct HttpReranker {
    client: reqwest::blocking::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    batch_size: usize,
}

impl HttpReranker {
    pub fn new(config: &RerankerConfig) -> Result<Self, RerankerError> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("http://localhost:8080");
        let model = config.model.as_deref().unwrap_or("BAAI/bge-reranker-v2-m3");
        let timeout = Duration::from_secs(config.timeout_secs);

        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| RerankerError::Http(format!("failed to build HTTP client: {e}")))?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
            model: model.to_string(),
            batch_size: config.batch_size,
        })
    }

    fn build_request(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let req = self.client.post(url);
        if let Some(ref key) = self.api_key {
            req.bearer_auth(key)
        } else {
            req
        }
    }
}

#[derive(Serialize)]
struct RerankRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_n: Option<usize>,
}

#[derive(Deserialize)]
struct RerankResponse {
    results: Vec<RerankResultItem>,
}

#[derive(Deserialize)]
struct RerankResultItem {
    index: usize,
    relevance_score: f64,
}

impl Reranker for HttpReranker {
    fn rerank(
        &self,
        query: &str,
        documents: &[RerankDocument],
    ) -> Result<Vec<RerankScore>, RerankerError> {
        if documents.is_empty() {
            return Ok(vec![]);
        }

        let mut all_scores: Vec<RerankScore> = Vec::with_capacity(documents.len());

        for chunk_start in (0..documents.len()).step_by(self.batch_size) {
            let chunk_end = (chunk_start + self.batch_size).min(documents.len());
            let chunk_docs: Vec<&str> = documents[chunk_start..chunk_end]
                .iter()
                .map(|d| d.text.as_str())
                .collect();

            let request = RerankRequest {
                model: &self.model,
                query,
                documents: chunk_docs,
                top_n: None,
            };

            let response = self.build_request("/rerank").json(&request).send()?;

            let status = response.status().as_u16();
            if status != 200 {
                let body = response.text().unwrap_or_default();
                return Err(RerankerError::Api {
                    status,
                    message: body,
                });
            }

            let parsed: RerankResponse = response
                .json()
                .map_err(|e| RerankerError::Json(e.to_string()))?;

            for item in parsed.results {
                all_scores.push(RerankScore {
                    index: chunk_start + item.index,
                    score: item.relevance_score,
                });
            }
        }

        Ok(all_scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubReranker {
        scores: Vec<f64>,
    }

    impl Reranker for StubReranker {
        fn rerank(
            &self,
            _query: &str,
            documents: &[RerankDocument],
        ) -> Result<Vec<RerankScore>, RerankerError> {
            let results: Vec<RerankScore> = documents
                .iter()
                .enumerate()
                .map(|(i, _)| RerankScore {
                    index: i,
                    score: self.scores.get(i).copied().unwrap_or(0.0),
                })
                .collect();
            Ok(results)
        }
    }

    #[test]
    fn test_stub_reranker_returns_scores() {
        let reranker = StubReranker {
            scores: vec![0.1, 0.9, 0.5],
        };
        let docs = vec![
            RerankDocument { text: "low".into() },
            RerankDocument {
                text: "high".into(),
            },
            RerankDocument { text: "mid".into() },
        ];
        let results = reranker.rerank("query", &docs).unwrap();
        assert_eq!(results.len(), 3);
        assert!((results[1].score - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_stub_reranker_empty() {
        let reranker = StubReranker { scores: vec![] };
        let results = reranker.rerank("query", &[]).unwrap();
        assert!(results.is_empty());
    }
}
