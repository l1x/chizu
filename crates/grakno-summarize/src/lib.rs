pub mod client;
pub mod config;
pub mod embedding;
pub mod error;
pub mod prompt;
pub mod summarizer;

pub use config::SummarizeConfig;
pub use embedding::{
    embed_entities_simple, EmbedOptions, EmbedStats, EmbeddingClient, SearchResult,
    SimpleEmbedOptions,
};
pub use error::SummarizeError;
pub use summarizer::{summarize_graph, SummarizeStats};
