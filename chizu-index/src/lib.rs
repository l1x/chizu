//! Chizu Index - Repository indexing pipeline and adapters.

pub mod adapter;
pub mod cleanup;
pub mod embedder;
pub mod error;
pub mod indexer;
pub mod ownership;
pub mod registry;
pub mod summarizer;
pub mod walk;

pub use error::{IndexError, Result};
pub use indexer::{IndexPipeline, IndexStats};
pub use ownership::{
    assign_ownership, discover_all_components, discover_cargo_components, discover_npm_components,
};
pub use registry::ComponentRegistry;
pub use walk::{FileWalker, WalkedFile};
