pub mod client;
pub mod config;
pub mod embedding;
pub mod error;
pub mod prompt;
pub mod summarizer;

pub use config::SummarizeConfig;
pub use embedding::{EmbedOptions, EmbedStats, EmbeddingClient, SearchResult};
pub use error::SummarizeError;
pub use summarizer::{summarize_graph, SummarizeStats};
