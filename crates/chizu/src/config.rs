//! Configuration management for Chizu.
//!
//! Supports workspace-local `.chizu.toml` files with validation.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Errors that can occur when loading or validating configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid TOML syntax: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("configuration validation failed:\n{0}")]
    Validation(String),
}

/// Chizu configuration root.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Indexing configuration.
    #[serde(default)]
    pub index: IndexConfig,

    /// Query configuration.
    #[serde(default)]
    pub query: QueryConfig,

    /// LLM/summarization configuration.
    #[serde(default)]
    pub llm: LlmConfig,

    /// Embedding configuration for vector search.
    #[serde(default)]
    pub embedding: EmbeddingConfig,
}

impl Config {
    /// Load configuration from a `.chizu.toml` file.
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    pub fn load(path: &Path) -> Result<Option<Self>, ConfigError> {
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;

        Ok(Some(config))
    }

    /// Find and load configuration by walking up from the given directory.
    pub fn find_from(start: &Path) -> Result<Option<(Self, std::path::PathBuf)>, ConfigError> {
        let mut current: Option<std::path::PathBuf> = Some(start.to_path_buf());

        while let Some(dir) = current {
            let config_path = dir.join(".chizu.toml");
            if let Some(config) = Self::load(&config_path)? {
                return Ok(Some((config, config_path)));
            }
            current = dir.parent().map(|p| p.to_path_buf());
        }

        Ok(None)
    }

    /// Validate configuration values.
    fn validate(&self) -> Result<(), ConfigError> {
        let mut errors = Vec::new();

        // Validate index config
        if self.index.parallel_workers == 0 {
            errors.push("index.parallel_workers must be at least 1".to_string());
        }
        if self.index.parallel_workers > 64 {
            errors.push("index.parallel_workers must be at most 64".to_string());
        }

        // Validate query config
        if self.query.default_limit == 0 {
            errors.push("query.default_limit must be at least 1".to_string());
        }
        if self.query.default_limit > 1000 {
            errors.push("query.default_limit must be at most 1000".to_string());
        }

        // Validate rerank weights sum to 1.0
        let weight_sum = self.query.rerank_weights.task_route
            + self.query.rerank_weights.keyword
            + self.query.rerank_weights.name_match
            + self.query.rerank_weights.vector
            + self.query.rerank_weights.kind_preference
            + self.query.rerank_weights.exported
            + self.query.rerank_weights.path_match;

        if (weight_sum - 1.0).abs() > 0.001 {
            errors.push(format!(
                "query.rerank_weights must sum to 1.0, got {:.3}",
                weight_sum
            ));
        }

        // Validate individual weights are non-negative
        let weights = [
            ("task_route", self.query.rerank_weights.task_route),
            ("keyword", self.query.rerank_weights.keyword),
            ("name_match", self.query.rerank_weights.name_match),
            ("vector", self.query.rerank_weights.vector),
            ("kind_preference", self.query.rerank_weights.kind_preference),
            ("exported", self.query.rerank_weights.exported),
            ("path_match", self.query.rerank_weights.path_match),
        ];

        for (name, value) in &weights {
            if *value < 0.0 {
                errors.push(format!("query.rerank_weights.{name} must be non-negative"));
            }
        }

        // Validate LLM config
        if self.llm.timeout_secs == 0 {
            errors.push("llm.timeout_secs must be at least 1".to_string());
        }
        if self.llm.timeout_secs > 300 {
            errors.push("llm.timeout_secs must be at most 300".to_string());
        }

        if self.llm.retry_attempts > 10 {
            errors.push("llm.retry_attempts must be at most 10".to_string());
        }

        if self.llm.max_tokens == 0 {
            errors.push("llm.max_tokens must be at least 1".to_string());
        }

        if !(0.0..=2.0).contains(&self.llm.temperature) {
            errors.push("llm.temperature must be between 0.0 and 2.0".to_string());
        }

        if !errors.is_empty() {
            return Err(ConfigError::Validation(errors.join("\n")));
        }

        Ok(())
    }

    /// Generate a default configuration file with comments.
    pub fn default_with_comments() -> String {
        r#"# Chizu configuration file
# Place this file in your workspace root as `.chizu.toml`

[index]
# File patterns to exclude from indexing (glob patterns)
exclude_patterns = [
    "**/target/**",
    "**/.git/**",
    "**/node_modules/**",
    "**/*.db",
    "**/*.db.usearch",
]

# Number of parallel workers for indexing (1-64)
parallel_workers = 4

[query]
# Default number of results for query plans (1-1000)
default_limit = 15

# Reranking weights (must sum to 1.0)
# These control how different signals contribute to result ranking.
[query.rerank_weights]
task_route = 0.30      # Task route priority signal
keyword = 0.20         # Keyword matching
name_match = 0.15      # Entity name matching
vector = 0.20          # Vector similarity (when embeddings enabled)
kind_preference = 0.05 # Preferred entity kinds for query category
exported = 0.05        # Bonus for exported/public entities
path_match = 0.05      # File path matching

[llm]
# Base URL for OpenAI-compatible API (e.g., Ollama, OpenAI)
base_url = "http://localhost:11434/v1"

# API key for authentication (empty for local endpoints like Ollama)
api_key = ""

# Default model for summarization
default_model = "llama3:8b"

# API timeout in seconds (1-300)
timeout_secs = 120

# Number of retry attempts for failed requests (0-10)
retry_attempts = 3

# Default max tokens for summarization
max_tokens = 512

# Sampling temperature (0.0-2.0, lower = more deterministic)
temperature = 0.2

[embedding]
# Enable automatic embedding generation during indexing
enabled = false

# Provider: "ollama" or "openai"
provider = "ollama"

# Base URL for the embedding API
# For Ollama: http://localhost:11434/v1
# For OpenAI: https://api.openai.com/v1
base_url = "http://localhost:11434/v1"

# API key (not needed for Ollama, required for OpenAI)
api_key = ""

# Model to use for embeddings
# Ollama examples: "nomic-embed-text", "mxbai-embed-large"
# OpenAI examples: "text-embedding-3-small", "text-embedding-3-large"
model = "nomic-embed-text"

# Embedding dimensions (must match the model)
dimensions = 768

# Batch size for embedding requests
batch_size = 32

# Timeout in seconds
timeout_secs = 120
"#
        .to_string()
    }
}

/// Indexing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    /// File patterns to exclude from indexing.
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,

    /// Number of parallel indexing workers.
    #[serde(default = "default_parallel_workers")]
    pub parallel_workers: usize,
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        "**/target/**".to_string(),
        "**/.git/**".to_string(),
        "**/node_modules/**".to_string(),
        "**/*.db".to_string(),
        "**/*.db.usearch".to_string(),
    ]
}

fn default_parallel_workers() -> usize {
    4
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            exclude_patterns: default_exclude_patterns(),
            parallel_workers: default_parallel_workers(),
        }
    }
}

/// Query configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryConfig {
    /// Default limit for query results.
    #[serde(default = "default_query_limit")]
    pub default_limit: usize,

    /// Reranking weights.
    #[serde(default)]
    pub rerank_weights: RerankWeights,
}

fn default_query_limit() -> usize {
    15
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            default_limit: default_query_limit(),
            rerank_weights: RerankWeights::default(),
        }
    }
}

/// Reranking weights configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RerankWeights {
    #[serde(default = "default_weight_task_route")]
    pub task_route: f64,
    #[serde(default = "default_weight_keyword")]
    pub keyword: f64,
    #[serde(default = "default_weight_name_match")]
    pub name_match: f64,
    #[serde(default = "default_weight_vector")]
    pub vector: f64,
    #[serde(default = "default_weight_kind_preference")]
    pub kind_preference: f64,
    #[serde(default = "default_weight_exported")]
    pub exported: f64,
    #[serde(default = "default_weight_path_match")]
    pub path_match: f64,
}

fn default_weight_task_route() -> f64 {
    0.30
}
fn default_weight_keyword() -> f64 {
    0.20
}
fn default_weight_name_match() -> f64 {
    0.15
}
fn default_weight_vector() -> f64 {
    0.20
}
fn default_weight_kind_preference() -> f64 {
    0.05
}
fn default_weight_exported() -> f64 {
    0.05
}
fn default_weight_path_match() -> f64 {
    0.05
}

impl Default for RerankWeights {
    fn default() -> Self {
        Self {
            task_route: default_weight_task_route(),
            keyword: default_weight_keyword(),
            name_match: default_weight_name_match(),
            vector: default_weight_vector(),
            kind_preference: default_weight_kind_preference(),
            exported: default_weight_exported(),
            path_match: default_weight_path_match(),
        }
    }
}

/// LLM configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    /// Base URL for the OpenAI-compatible API.
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// API key for authentication (empty string for local endpoints).
    #[serde(default = "default_api_key")]
    pub api_key: String,

    /// Default model for LLM operations.
    #[serde(default = "default_model")]
    pub default_model: String,

    /// API timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Number of retry attempts.
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,

    /// Max tokens for generation.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// Sampling temperature.
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_base_url() -> String {
    "http://localhost:11434/v1".to_string()
}
fn default_api_key() -> String {
    String::new()
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}
fn default_timeout() -> u64 {
    60
}
fn default_retry_attempts() -> u32 {
    3
}
fn default_max_tokens() -> u32 {
    512
}
fn default_temperature() -> f32 {
    0.2
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: default_api_key(),
            default_model: default_model(),
            timeout_secs: default_timeout(),
            retry_attempts: default_retry_attempts(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
        }
    }
}

/// Embedding configuration for vector search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingConfig {
    /// Whether to automatically generate embeddings during indexing.
    #[serde(default = "default_embedding_enabled")]
    pub enabled: bool,

    /// Provider type: "ollama" or "openai".
    #[serde(default = "default_embedding_provider")]
    pub provider: String,

    /// Base URL for the embedding API.
    #[serde(default = "default_embedding_base_url")]
    pub base_url: String,

    /// API key (required for OpenAI, not for Ollama).
    #[serde(default)]
    pub api_key: String,

    /// Model to use for embeddings.
    #[serde(default = "default_embedding_model")]
    pub model: String,

    /// Embedding dimensions (must match model).
    #[serde(default = "default_embedding_dimensions")]
    pub dimensions: usize,

    /// Batch size for embedding requests.
    #[serde(default = "default_embedding_batch_size")]
    pub batch_size: usize,

    /// Timeout in seconds.
    #[serde(default = "default_embedding_timeout")]
    pub timeout_secs: u64,
}

fn default_embedding_enabled() -> bool {
    false
}

fn default_embedding_provider() -> String {
    "ollama".to_string()
}

fn default_embedding_base_url() -> String {
    "http://localhost:11434/v1".to_string()
}

fn default_embedding_model() -> String {
    "nomic-embed-text".to_string()
}

fn default_embedding_dimensions() -> usize {
    768
}

fn default_embedding_batch_size() -> usize {
    32
}

fn default_embedding_timeout() -> u64 {
    120
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: default_embedding_enabled(),
            provider: default_embedding_provider(),
            base_url: default_embedding_base_url(),
            api_key: default_embedding_api_key(),
            model: default_embedding_model(),
            dimensions: default_embedding_dimensions(),
            batch_size: default_embedding_batch_size(),
            timeout_secs: default_embedding_timeout(),
        }
    }
}

fn default_embedding_api_key() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn config_round_trip() {
        let config = Config::default();
        let mut file = NamedTempFile::new().unwrap();

        // Write config as TOML
        let content = toml::to_string_pretty(&config).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let loaded = Config::load(file.path()).unwrap().unwrap();

        assert_eq!(loaded.index.parallel_workers, config.index.parallel_workers);
        assert_eq!(loaded.query.default_limit, config.query.default_limit);
        assert_eq!(
            loaded.query.rerank_weights.task_route,
            config.query.rerank_weights.task_route
        );
        assert_eq!(loaded.llm.default_model, config.llm.default_model);
    }

    #[test]
    fn config_load_missing_file() {
        let result = Config::load(Path::new("/nonexistent/.chizu.toml")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn config_validation_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_validation_zero_workers() {
        let config = Config {
            index: IndexConfig {
                parallel_workers: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("parallel_workers must be at least 1"));
    }

    #[test]
    fn config_validation_weight_sum() {
        let config = Config {
            query: QueryConfig {
                rerank_weights: RerankWeights {
                    task_route: 0.5,
                    keyword: 0.5,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("must sum to 1.0"));
    }

    #[test]
    fn config_validation_negative_weight() {
        let config = Config {
            query: QueryConfig {
                rerank_weights: RerankWeights {
                    task_route: -0.1,
                    keyword: 0.3,
                    name_match: 0.15,
                    vector: 0.20,
                    kind_preference: 0.05,
                    exported: 0.05,
                    path_match: 0.35, // Adjust to sum to 1.0
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("must be non-negative"));
    }

    #[test]
    fn config_parse_valid_toml() {
        let toml = r#"
[index]
parallel_workers = 8
exclude_patterns = ["**/*.tmp"]

[query]
default_limit = 25

[query.rerank_weights]
task_route = 0.40
keyword = 0.30
name_match = 0.10
vector = 0.10
kind_preference = 0.05
exported = 0.05
path_match = 0.00

[llm]
default_model = "gpt-4"
timeout_secs = 120
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(toml.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap().unwrap();
        assert_eq!(config.index.parallel_workers, 8);
        assert_eq!(config.query.default_limit, 25);
        assert_eq!(config.query.rerank_weights.task_route, 0.40);
        assert_eq!(config.llm.default_model, "gpt-4");
        assert_eq!(config.llm.timeout_secs, 120);
    }

    #[test]
    fn config_parse_unknown_field() {
        let toml = r#"
[index]
unknown_field = "value"
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(toml.as_bytes()).unwrap();

        let result = Config::load(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn config_default_with_comments_contains_examples() {
        let content = Config::default_with_comments();
        assert!(content.contains("[index]"));
        assert!(content.contains("[query]"));
        assert!(content.contains("[llm]"));
        assert!(content.contains("parallel_workers"));
        assert!(content.contains("rerank_weights"));
    }
}
