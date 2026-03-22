pub mod discover;
pub mod error;
pub mod id;
pub mod indexer;
pub mod parser;

pub use error::IndexError;
pub use indexer::{index_project, IndexStats};
