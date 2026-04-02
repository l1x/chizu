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
    /// Visualization configuration
    pub visualize: VisualizeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            index: IndexConfig::default(),
            search: SearchConfig::default(),
            providers: default_providers(),
            summary: SummaryConfig::default(),
            embedding: EmbeddingConfig::default(),
            visualize: VisualizeConfig::default(),
        }
    }
}

impl Config {
    /// Serialize configuration to a TOML string.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(ConfigError::Serialize)
    }

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
        self.search.validate_weights()?;

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

        // Summary/embedding must use the same provider (single-provider constraint).
        if let (Some(s), Some(e)) = (&self.summary.provider, &self.embedding.provider) {
            if s != e {
                return Err(ConfigError::InvalidParam(format!(
                    "summary provider '{}' and embedding provider '{}' must be the same",
                    s, e
                )));
            }
        }

        if let Some(t) = self.summary.temperature {
            if !(0.0..=2.0).contains(&t) {
                return Err(ConfigError::InvalidParam(format!(
                    "summary.temperature must be 0.0..2.0, got {t}"
                )));
            }
        }

        if let Some(b) = self.summary.batch_size {
            if b == 0 {
                return Err(ConfigError::InvalidParam(
                    "summary.batch_size must be > 0".into(),
                ));
            }
        }

        if let Some(d) = self.embedding.dimensions {
            if d == 0 {
                return Err(ConfigError::InvalidParam(
                    "embedding.dimensions must be > 0".into(),
                ));
            }
        }

        if let Some(b) = self.embedding.batch_size {
            if b == 0 {
                return Err(ConfigError::InvalidParam(
                    "embedding.batch_size must be > 0".into(),
                ));
            }
        }

        if let Some(template) = &self.visualize.editor_link
            && template.trim().is_empty()
        {
            return Err(ConfigError::InvalidParam(
                "visualize.editor_link must not be empty".into(),
            ));
        }

        Ok(())
    }
}

/// Indexing configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    pub exclude_patterns: Vec<String>,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            exclude_patterns: default_exclude_patterns(),
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
    /// Number of entities to include in each summary LLM request
    pub batch_size: Option<usize>,
    /// Number of concurrent LLM calls (default 1)
    pub concurrency: Option<usize>,
    /// Only summarize exported (pub) symbols (default true)
    pub exported_only: Option<bool>,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            provider: Some("ollama".to_string()),
            model: Some("llama3:8b".to_string()),
            max_tokens: Some(512),
            temperature: Some(0.2),
            batch_size: Some(4),
            concurrency: Some(1),
            exported_only: Some(true),
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

/// Visualization configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VisualizeConfig {
    /// Optional editor URL template for "Open in editor" links.
    /// Available placeholders: {abs_path}, {repo_path}, {line}, {column}, {entity_id}
    pub editor_link: Option<String>,
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    Serialize(toml::ser::Error),
    #[error("rerank weights must sum to 1.0, got {sum}")]
    InvalidWeights { sum: f64 },
    #[error("missing provider: {0}")]
    MissingProvider(String),
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
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
exclude_patterns = ["**/vendor/**"]
"#;
        let config = Config::from_toml(toml).unwrap();
        assert!(
            config
                .index
                .exclude_patterns
                .contains(&"**/vendor/**".to_string())
        );
        // Defaults preserved
        assert_eq!(config.search.default_limit, 15);
    }

    #[test]
    fn test_to_toml_roundtrip() {
        let original = Config::default();
        let toml_str = original.to_toml().unwrap();

        // The serialized TOML must parse back into a valid config
        let parsed = Config::from_toml(&toml_str).unwrap();
        assert_eq!(parsed.search.default_limit, original.search.default_limit);
        assert_eq!(
            parsed.search.rerank_weights.keyword,
            original.search.rerank_weights.keyword
        );
        assert!(parsed.providers.contains_key("ollama"));
        assert_eq!(parsed.embedding.dimensions, original.embedding.dimensions);
        assert_eq!(parsed.visualize.editor_link, original.visualize.editor_link);
    }

    #[test]
    fn test_to_toml_produces_valid_toml() {
        let config = Config::default();
        let toml_str = config.to_toml().unwrap();

        // Must be parseable as raw TOML
        let _: toml::Value = toml::from_str(&toml_str).unwrap();
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

    #[test]
    fn test_invalid_temperature() {
        let toml = r#"
[summary]
temperature = 3.0
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_summary_batch_size_rejected() {
        let toml = r#"
[summary]
batch_size = 0
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_dimensions_rejected() {
        let toml = r#"
[embedding]
dimensions = 0
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_batch_size_rejected() {
        let toml = r#"
[embedding]
batch_size = 0
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_different_providers_rejected() {
        let toml = r#"
[providers.a]
base_url = "http://localhost:1"
[providers.b]
base_url = "http://localhost:2"
[summary]
provider = "a"
[embedding]
provider = "b"
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_visualize_editor_link_config() {
        let toml = r#"
[visualize]
editor_link = "vscode://file/{abs_path}:{line}:{column}"
"#;
        let config = Config::from_toml(toml).unwrap();
        assert_eq!(
            config.visualize.editor_link,
            Some("vscode://file/{abs_path}:{line}:{column}".to_string())
        );
    }

    #[test]
    fn test_empty_visualize_editor_link_rejected() {
        let toml = r#"
[visualize]
editor_link = "   "
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }
}
