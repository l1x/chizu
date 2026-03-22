use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingRecord {
    pub entity_id: String,
    pub model: String,
    pub dimensions: i64,
    pub vector_ref: Option<String>,
    pub updated_at: String,
}
