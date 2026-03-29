use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Chizu configuration file (.chizu.toml)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Indexing configuration
    pub index: IndexConfig,
    /// Search configuration
    pub search: SearchConfig,
    /// Provider configurations
    pub providers: HashMap<String, ProviderConfig>,
    /// Summary generation configuration
    pub summary: SummaryConfig,
    /// Embedding configuration
    pub embedding: EmbeddingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            index: IndexConfig::default(),
            search: SearchConfig::default(),
            providers: default_providers(),
            summary: SummaryConfig::default(),
            embedding: EmbeddingConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML string.
    pub fn from_toml(s: &str) -> Result<Self, ConfigError> {
        let mut config: Config = toml::from_str(s)?;

        // Merge default providers with user-defined ones
        let defaults = default_providers();
        for (key, value) in defaults {
            config.providers.entry(key).or_insert(value);
        }

        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate rerank weights sum to 1.0
        self.search.validate_weights()?;

        // Validate that referenced providers exist
        if let Some(ref provider) = self.summary.provider
            && !self.providers.contains_key(provider)
        {
            return Err(ConfigError::MissingProvider(provider.clone()));
        }
        if let Some(ref provider) = self.embedding.provider
            && !self.providers.contains_key(provider)
        {
            return Err(ConfigError::MissingProvider(provider.clone()));
        }

        Ok(())
    }
}

/// Indexing configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    /// File patterns to exclude
    pub exclude_patterns: Vec<String>,
    /// Number of parallel workers (defaults to CPU count)
    pub parallel_workers: Option<usize>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            exclude_patterns: default_exclude_patterns(),
            parallel_workers: None,
        }
    }
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        "**/target/**".to_string(),
        "**/.git/**".to_string(),
        "**/node_modules/**".to_string(),
        "**/.venv/**".to_string(),
        "**/fuzz/**".to_string(),
        "**/*.lock".to_string(),
    ]
}

/// Search configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Default result limit
    pub default_limit: usize,
    /// Reranking weights
    pub rerank_weights: RerankWeights,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: 15,
            rerank_weights: RerankWeights::default(),
        }
    }
}

impl SearchConfig {
    /// Validate that weights sum to 1.0
    pub fn validate_weights(&self) -> Result<(), ConfigError> {
        let sum = self.rerank_weights.sum();
        let epsilon = 0.001;
        if (sum - 1.0).abs() > epsilon {
            return Err(ConfigError::InvalidWeights { sum });
        }
        Ok(())
    }
}

/// Reranking weights (must sum to 1.0)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RerankWeights {
    pub task_route: f64,
    pub keyword: f64,
    pub name_match: f64,
    pub vector: f64,
    pub kind_preference: f64,
    pub exported: f64,
    pub path_match: f64,
}

impl Default for RerankWeights {
    fn default() -> Self {
        Self {
            task_route: 0.00,
            keyword: 0.25,
            name_match: 0.20,
            vector: 0.25,
            kind_preference: 0.10,
            exported: 0.10,
            path_match: 0.10,
        }
    }
}

impl RerankWeights {
    /// Sum of all weights
    pub fn sum(&self) -> f64 {
        self.task_route
            + self.keyword
            + self.name_match
            + self.vector
            + self.kind_preference
            + self.exported
            + self.path_match
    }
}

/// Provider connection configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// Base URL for the provider API
    pub base_url: String,
    /// API key (empty for local providers like Ollama)
    pub api_key: String,
    /// Timeout in seconds
    pub timeout_secs: u64,
    /// Number of retry attempts
    pub retry_attempts: u32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: String::new(),
            timeout_secs: 120,
            retry_attempts: 3,
        }
    }
}

fn default_providers() -> HashMap<String, ProviderConfig> {
    let mut providers = HashMap::new();
    providers.insert("ollama".to_string(), ProviderConfig::default());
    providers
}

/// Summary generation configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryConfig {
    /// Provider name (references providers.*)
    pub provider: Option<String>,
    /// Model to use
    pub model: Option<String>,
    /// Max tokens to generate
    pub max_tokens: Option<u32>,
    /// Temperature for generation
    pub temperature: Option<f64>,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            provider: Some("ollama".to_string()),
            model: Some("llama3:8b".to_string()),
            max_tokens: Some(512),
            temperature: Some(0.2),
        }
    }
}

/// Embedding configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    /// Provider name (references providers.*)
    pub provider: Option<String>,
    /// Model to use
    pub model: Option<String>,
    /// Number of dimensions
    pub dimensions: Option<u32>,
    /// Batch size for embedding generation
    pub batch_size: Option<usize>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: Some("ollama".to_string()),
            model: Some("nomic-embed-text-v2-moe:latest".to_string()),
            dimensions: Some(768),
            batch_size: Some(32),
        }
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("rerank weights must sum to 1.0, got {sum}")]
    InvalidWeights { sum: f64 },
    #[error("missing provider: {0}")]
    MissingProvider(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.search.default_limit, 15);
        assert!(config.providers.contains_key("ollama"));
    }

    #[test]
    fn test_config_from_toml() {
        let toml = r#"
[search]
default_limit = 20

[search.rerank_weights]
keyword = 0.30
name_match = 0.20
vector = 0.30
kind_preference = 0.10
exported = 0.05
path_match = 0.05
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.search.default_limit, 20);
        assert_eq!(config.search.rerank_weights.keyword, 0.30);
    }

    #[test]
    fn test_default_weights_sum_to_one() {
        let weights = RerankWeights::default();
        assert!((weights.sum() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_invalid_weights() {
        let toml = r#"
[search.rerank_weights]
keyword = 0.5
name_match = 0.5
vector = 0.5
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_provider() {
        let toml = r#"
[summary]
provider = "nonexistent"
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_partial_toml() {
        let toml = r#"
[index]
parallel_workers = 8
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(config.index.parallel_workers, Some(8));
        // Defaults preserved
        assert_eq!(config.search.default_limit, 15);
    }

    #[test]
    fn test_provider_config() {
        let toml = r#"
[providers.custom]
base_url = "https://api.example.com/v1"
api_key = "secret"
timeout_secs = 60
retry_attempts = 5
"#;
        let config = Config::from_toml(toml).unwrap();
        let custom = config.providers.get("custom").unwrap();
        assert_eq!(custom.base_url, "https://api.example.com/v1");
        assert_eq!(custom.api_key, "secret");
    }
}
