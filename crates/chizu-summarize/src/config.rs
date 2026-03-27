/// Configuration for the LLM summarization endpoint.
pub struct SummarizeConfig {
    /// Base URL for the OpenAI-compatible API (e.g. "https://api.openai.com/v1")
    pub base_url: String,
    /// API key for authentication ("" for keyless local endpoints)
    pub api_key: String,
    /// Model identifier (e.g. "gpt-4o-mini")
    pub model: String,
    /// Maximum tokens in the response
    pub max_tokens: u32,
    /// Sampling temperature
    pub temperature: f32,
}

impl SummarizeConfig {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            max_tokens: 512,
            temperature: 0.2,
        }
    }
}
