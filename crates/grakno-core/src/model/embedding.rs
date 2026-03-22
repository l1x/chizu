use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingRecord {
    pub entity_id: String,
    pub model: String,
    pub dimensions: i64,
    pub vector: Vec<f32>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorSearchResult {
    pub entity_id: String,
    pub distance: f32,
}
