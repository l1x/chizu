use crate::config::SummarizeConfig;
use crate::error::{Result, SummarizeError};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;

pub struct LlmClient {
    http: reqwest::blocking::Client,
    base_url: String,
    api_key: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF_SECS: u64 = 2;

impl LlmClient {
    pub fn new(config: &SummarizeConfig) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
        })
    }

    /// Send a chat completion request with system and user messages.
    /// Retries on 429 (rate-limit) and 5xx errors up to 3 times.
    pub fn chat(&self, system: &str, user: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                Message {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
            max_tokens: self.max_tokens,
            temperature: self.temperature,
        };

        let mut last_err: Option<SummarizeError> = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = Duration::from_secs(BASE_BACKOFF_SECS * u64::from(attempt));
                thread::sleep(backoff);
            }

            let mut req = self.http.post(&url).json(&body);
            if !self.api_key.is_empty() {
                req = req.bearer_auth(&self.api_key);
            }

            let response = match req.send() {
                Ok(r) => r,
                Err(e) => {
                    // Network errors are fatal — don't retry
                    return Err(SummarizeError::Http(e));
                }
            };

            let status = response.status().as_u16();
            if status == 200 {
                let chat_resp: ChatResponse = response.json()?;
                let content = chat_resp
                    .choices
                    .into_iter()
                    .next()
                    .map(|c| c.message.content)
                    .unwrap_or_default();
                return Ok(content);
            }

            let body_text = response.text().unwrap_or_default();

            if status == 429 || status >= 500 {
                last_err = Some(SummarizeError::Api {
                    status,
                    body: body_text,
                });
                continue;
            }

            // 4xx (not 429) — don't retry
            return Err(SummarizeError::Api {
                status,
                body: body_text,
            });
        }

        Err(last_err.unwrap_or_else(|| SummarizeError::Api {
            status: 0,
            body: "max retries exceeded".to_string(),
        }))
    }
}
