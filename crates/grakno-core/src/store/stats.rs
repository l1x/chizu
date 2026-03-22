#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphStats {
    pub entities: u64,
    pub edges: u64,
    pub files: u64,
    pub summaries: u64,
    pub task_routes: u64,
    pub embeddings: u64,
}
