use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub entity_id: String,
    pub short_summary: String,
    pub detailed_summary: Option<String>,
    pub keywords: Vec<String>,
    pub updated_at: String,
    pub source_hash: Option<String>,
}
