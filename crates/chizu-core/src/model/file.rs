use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub component_id: Option<String>,
    pub kind: String,
    pub hash: String,
    pub indexed: bool,
    pub ignore_reason: Option<String>,
}
