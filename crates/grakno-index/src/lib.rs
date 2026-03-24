pub mod discover;
pub mod error;
pub mod id;
pub mod indexer;
pub mod markdown;
pub mod mise;
pub mod parser;
pub mod parser_astro;
pub mod parser_ts;

pub use error::IndexError;
pub use indexer::{index_generic_project, index_project, IndexStats};
