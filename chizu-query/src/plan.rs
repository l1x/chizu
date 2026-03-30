use chizu_core::{EntityKind, TaskCategory};
use serde::Serialize;

/// A ranked reading plan returned by the search pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct ReadingPlan {
    pub query: String,
    pub category: TaskCategory,
    pub entries: Vec<PlanEntry>,
}

/// A single entry in the reading plan.
#[derive(Debug, Clone, Serialize)]
pub struct PlanEntry {
    pub rank: usize,
    pub entity_id: String,
    pub entity_kind: EntityKind,
    pub name: String,
    pub path: Option<String>,
    pub score: f64,
    pub is_context: bool,
    pub reasons: Vec<String>,
}

impl ReadingPlan {
    /// Serialize the plan to a JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serialize the plan to a human-readable text string.
    pub fn to_text(&self) -> String {
        let mut lines = vec![
            format!("Query: {}", self.query),
            format!("Category: {}", self.category),
            format!("Results: {}", self.entries.len()),
            String::from("---"),
        ];

        for entry in &self.entries {
            let context_tag = if entry.is_context { " [context]" } else { "" };
            lines.push(format!(
                "{}. {} ({}) -- {:.3}{}",
                entry.rank, entry.name, entry.entity_kind, entry.score, context_tag
            ));
            if let Some(ref path) = entry.path {
                lines.push(format!("   Path: {}", path));
            }
            lines.push(format!("   ID: {}", entry.entity_id));
            if !entry.reasons.is_empty() {
                lines.push(format!("   Why: {}", entry.reasons.join(", ")));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{EntityKind, TaskCategory};

    #[test]
    fn test_plan_to_json() {
        let plan = ReadingPlan {
            query: "how does routing work".into(),
            category: TaskCategory::Understand,
            entries: vec![PlanEntry {
                rank: 1,
                entity_id: "symbol::router.rs::handle".into(),
                entity_kind: EntityKind::Symbol,
                name: "handle".into(),
                path: Some("router.rs".into()),
                score: 0.95,
                is_context: false,
                reasons: vec!["name match".into()],
            }],
        };
        let json = plan.to_json().unwrap();
        assert!(json.contains("handle"));
        assert!(json.contains("0.95"));
    }

    #[test]
    fn test_plan_to_text() {
        let plan = ReadingPlan {
            query: "test".into(),
            category: TaskCategory::Test,
            entries: vec![PlanEntry {
                rank: 1,
                entity_id: "test::1".into(),
                entity_kind: EntityKind::Test,
                name: "test_foo".into(),
                path: None,
                score: 0.88,
                is_context: true,
                reasons: vec!["task route".into()],
            }],
        };
        let text = plan.to_text();
        assert!(text.contains("test_foo"));
        assert!(text.contains("[context]"));
        assert!(text.contains("0.880"));
    }
}
