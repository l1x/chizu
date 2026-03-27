use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRoute {
    pub task_name: String,
    pub entity_id: String,
    pub priority: i64,
}
