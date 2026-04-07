use chizu_core::{EntityKind, TaskCategory};
use serde::Serialize;

/// A ranked reading plan returned by the search pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct ReadingPlan {
    pub query: String,
    pub category: TaskCategory,
    pub entries: Vec<PlanEntry>,
    /// Total candidates before cutoff (None if cutoff was not applied).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_before_cutoff: Option<usize>,
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
    /// Score breakdown per signal (populated in verbose mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_breakdown: Option<ScoreBreakdown>,
}

/// Per-signal score breakdown for verbose output.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreBreakdown {
    pub keyword: f64,
    pub name_match: f64,
    pub path_match: f64,
    pub vector: f64,
    pub task_route: f64,
    pub kind_preference: f64,
    pub exported: f64,
}

impl ReadingPlan {
    /// Serialize the plan to a JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serialize the plan to a human-readable text string.
    pub fn to_text(&self) -> String {
        self.format_text(false)
    }

    /// Serialize with verbose score details.
    pub fn to_text_verbose(&self) -> String {
        self.format_text(true)
    }

    fn format_text(&self, verbose: bool) -> String {
        let mut lines = vec![
            format!("Query: {}", self.query),
            format!("Category: {}", self.category),
        ];

        if let Some(total) = self.total_before_cutoff {
            lines.push(format!(
                "Results: {} (cutoff from {})",
                self.entries.len(),
                total
            ));
        } else {
            lines.push(format!("Results: {}", self.entries.len()));
        }
        lines.push(String::from("---"));

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
            if verbose && let Some(ref bd) = entry.score_breakdown {
                lines.push(format!(
                    "   Scores: keyword={:.2}, name={:.2}, path={:.2}, vector={:.2}, task_route={:.2}, kind={:.2}, exported={:.2}",
                    bd.keyword, bd.name_match, bd.path_match, bd.vector,
                    bd.task_route, bd.kind_preference, bd.exported
                ));
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
                score_breakdown: None,
            }],
            total_before_cutoff: None,
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
                score_breakdown: None,
            }],
            total_before_cutoff: None,
        };
        let text = plan.to_text();
        assert!(text.contains("test_foo"));
        assert!(text.contains("[context]"));
        assert!(text.contains("0.880"));
    }

    #[test]
    fn test_plan_to_text_with_cutoff() {
        let plan = ReadingPlan {
            query: "test".into(),
            category: TaskCategory::General,
            entries: vec![PlanEntry {
                rank: 1,
                entity_id: "a".into(),
                entity_kind: EntityKind::Symbol,
                name: "foo".into(),
                path: None,
                score: 0.9,
                is_context: false,
                reasons: vec![],
                score_breakdown: None,
            }],
            total_before_cutoff: Some(5),
        };
        let text = plan.to_text();
        assert!(text.contains("Results: 1 (cutoff from 5)"));
    }

    #[test]
    fn test_plan_verbose_shows_breakdown() {
        let plan = ReadingPlan {
            query: "test".into(),
            category: TaskCategory::General,
            entries: vec![PlanEntry {
                rank: 1,
                entity_id: "a".into(),
                entity_kind: EntityKind::Symbol,
                name: "foo".into(),
                path: None,
                score: 0.55,
                is_context: false,
                reasons: vec![],
                score_breakdown: Some(ScoreBreakdown {
                    keyword: 0.80,
                    name_match: 0.0,
                    path_match: 0.0,
                    vector: 0.90,
                    task_route: 0.0,
                    kind_preference: 1.0,
                    exported: 1.0,
                }),
            }],
            total_before_cutoff: None,
        };
        let text = plan.to_text_verbose();
        assert!(text.contains("Scores:"));
        assert!(text.contains("keyword=0.80"));
        assert!(text.contains("vector=0.90"));
    }

    #[test]
    fn test_plan_non_verbose_hides_breakdown() {
        let plan = ReadingPlan {
            query: "test".into(),
            category: TaskCategory::General,
            entries: vec![PlanEntry {
                rank: 1,
                entity_id: "a".into(),
                entity_kind: EntityKind::Symbol,
                name: "foo".into(),
                path: None,
                score: 0.55,
                is_context: false,
                reasons: vec![],
                score_breakdown: Some(ScoreBreakdown {
                    keyword: 0.80,
                    name_match: 0.0,
                    path_match: 0.0,
                    vector: 0.90,
                    task_route: 0.0,
                    kind_preference: 1.0,
                    exported: 1.0,
                }),
            }],
            total_before_cutoff: None,
        };
        let text = plan.to_text();
        assert!(!text.contains("Scores:"));
    }
}
