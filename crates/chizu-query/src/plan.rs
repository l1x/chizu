use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingPlan {
    pub query: String,
    pub category: String,
    pub items: Vec<ReadingPlanItem>,
    pub candidates_considered: usize,
    pub used_vector_search: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingPlanItem {
    pub entity_id: String,
    pub name: String,
    pub kind: String,
    pub path: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub short_summary: String,
    pub score: f64,
    pub reasons: Vec<String>,
    pub is_context: bool,
    pub context_via: Option<String>,
}

impl ReadingPlan {
    pub fn display(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("Reading plan for: \"{}\"\n", self.query));
        out.push_str(&format!("Category: {}\n", self.category));
        out.push_str(&format!(
            "Candidates considered: {} | Vector search: {}\n",
            self.candidates_considered,
            if self.used_vector_search { "yes" } else { "no" },
        ));
        out.push_str(&format!("Results: {}\n", self.items.len()));
        out.push('\n');

        for (i, item) in self.items.iter().enumerate() {
            let marker = if item.is_context { "  " } else { "" };
            let prefix = if item.is_context {
                format!("   {marker}ctx")
            } else {
                format!("{:>4}", i + 1)
            };

            let location = match (&item.path, item.line_start) {
                (Some(p), Some(l)) => format!("{p}:{l}"),
                (Some(p), None) => p.clone(),
                _ => String::new(),
            };

            out.push_str(&format!(
                "{}. [{}] {} (score: {:.3})\n",
                prefix, item.kind, item.name, item.score,
            ));
            if !location.is_empty() {
                out.push_str(&format!("      {location}\n"));
            }
            if !item.short_summary.is_empty() {
                out.push_str(&format!("      {}\n", item.short_summary));
            }
            if !item.reasons.is_empty() {
                out.push_str(&format!("      reasons: {}\n", item.reasons.join(", ")));
            }
            if let Some(ref via) = item.context_via {
                out.push_str(&format!("      context via: {via}\n"));
            }
            out.push('\n');
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(
        name: &str,
        kind: &str,
        path: Option<&str>,
        line_start: Option<i64>,
        score: f64,
        is_context: bool,
        context_via: Option<&str>,
    ) -> ReadingPlanItem {
        ReadingPlanItem {
            entity_id: format!("entity::{name}"),
            name: name.to_string(),
            kind: kind.to_string(),
            path: path.map(String::from),
            line_start,
            line_end: line_start.map(|l| l + 20),
            short_summary: format!("Summary of {name}"),
            score,
            reasons: vec!["name_match".to_string(), "exported".to_string()],
            is_context,
            context_via: context_via.map(String::from),
        }
    }

    fn make_plan() -> ReadingPlan {
        ReadingPlan {
            query: "how does auth work".to_string(),
            category: "understand".to_string(),
            items: vec![
                make_item(
                    "AuthService",
                    "symbol",
                    Some("src/auth.rs"),
                    Some(10),
                    0.85,
                    false,
                    None,
                ),
                make_item(
                    "Token",
                    "symbol",
                    Some("src/token.rs"),
                    Some(5),
                    0.60,
                    false,
                    None,
                ),
                make_item(
                    "Config",
                    "symbol",
                    Some("src/config.rs"),
                    None,
                    0.30,
                    true,
                    Some("entity::AuthService"),
                ),
            ],
            candidates_considered: 42,
            used_vector_search: true,
        }
    }

    // --- JSON serialization round-trip ---

    #[test]
    fn serde_round_trip() {
        let plan = make_plan();
        let json = serde_json::to_string(&plan).unwrap();
        let back: ReadingPlan = serde_json::from_str(&json).unwrap();

        assert_eq!(back.query, plan.query);
        assert_eq!(back.category, plan.category);
        assert_eq!(back.candidates_considered, plan.candidates_considered);
        assert_eq!(back.used_vector_search, plan.used_vector_search);
        assert_eq!(back.items.len(), plan.items.len());
    }

    #[test]
    fn serde_preserves_all_item_fields() {
        let plan = make_plan();
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let back: ReadingPlan = serde_json::from_str(&json).unwrap();

        let orig = &plan.items[0];
        let restored = &back.items[0];
        assert_eq!(restored.entity_id, orig.entity_id);
        assert_eq!(restored.name, orig.name);
        assert_eq!(restored.kind, orig.kind);
        assert_eq!(restored.path, orig.path);
        assert_eq!(restored.line_start, orig.line_start);
        assert_eq!(restored.line_end, orig.line_end);
        assert_eq!(restored.short_summary, orig.short_summary);
        assert!((restored.score - orig.score).abs() < 1e-10);
        assert_eq!(restored.reasons, orig.reasons);
        assert_eq!(restored.is_context, orig.is_context);
        assert_eq!(restored.context_via, orig.context_via);
    }

    #[test]
    fn serde_context_item_preserves_context_via() {
        let plan = make_plan();
        let json = serde_json::to_string(&plan).unwrap();
        let back: ReadingPlan = serde_json::from_str(&json).unwrap();

        let ctx_item = &back.items[2];
        assert!(ctx_item.is_context);
        assert_eq!(ctx_item.context_via.as_deref(), Some("entity::AuthService"));
    }

    #[test]
    fn serde_empty_plan() {
        let plan = ReadingPlan {
            query: "".to_string(),
            category: "general".to_string(),
            items: vec![],
            candidates_considered: 0,
            used_vector_search: false,
        };
        let json = serde_json::to_string(&plan).unwrap();
        let back: ReadingPlan = serde_json::from_str(&json).unwrap();
        assert!(back.items.is_empty());
        assert_eq!(back.candidates_considered, 0);
    }

    // --- display() ---

    #[test]
    fn display_contains_query() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("how does auth work"));
    }

    #[test]
    fn display_contains_category() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("Category: understand"));
    }

    #[test]
    fn display_contains_candidates_count() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("Candidates considered: 42"));
    }

    #[test]
    fn display_vector_search_yes() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("Vector search: yes"));
    }

    #[test]
    fn display_vector_search_no() {
        let mut plan = make_plan();
        plan.used_vector_search = false;
        let out = plan.display();
        assert!(out.contains("Vector search: no"));
    }

    #[test]
    fn display_contains_results_count() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("Results: 3"));
    }

    #[test]
    fn display_numbered_items_have_rank() {
        let plan = make_plan();
        let out = plan.display();
        // Non-context items should have numbers like "   1."
        assert!(out.contains("1. [symbol] AuthService"));
        assert!(out.contains("2. [symbol] Token"));
    }

    #[test]
    fn display_context_items_marked_ctx() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("ctx. [symbol] Config"));
    }

    #[test]
    fn display_shows_location_with_line() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("src/auth.rs:10"));
    }

    #[test]
    fn display_shows_location_without_line() {
        let plan = make_plan();
        let out = plan.display();
        // Config has path but no line_start -> just path
        assert!(out.contains("src/config.rs"));
    }

    #[test]
    fn display_shows_summary() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("Summary of AuthService"));
    }

    #[test]
    fn display_shows_reasons() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("reasons: name_match, exported"));
    }

    #[test]
    fn display_shows_context_via() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("context via: entity::AuthService"));
    }

    #[test]
    fn display_shows_score() {
        let plan = make_plan();
        let out = plan.display();
        assert!(out.contains("score: 0.850"));
    }

    #[test]
    fn display_empty_plan() {
        let plan = ReadingPlan {
            query: "nothing".to_string(),
            category: "general".to_string(),
            items: vec![],
            candidates_considered: 0,
            used_vector_search: false,
        };
        let out = plan.display();
        assert!(out.contains("Results: 0"));
        // No items, so no numbered lines
        assert!(!out.contains("1."));
    }

    #[test]
    fn display_item_no_path_no_location_line() {
        let item = ReadingPlanItem {
            entity_id: "e::X".to_string(),
            name: "X".to_string(),
            kind: "symbol".to_string(),
            path: None,
            line_start: None,
            line_end: None,
            short_summary: String::new(),
            score: 0.1,
            reasons: vec![],
            is_context: false,
            context_via: None,
        };
        let plan = ReadingPlan {
            query: "q".to_string(),
            category: "general".to_string(),
            items: vec![item],
            candidates_considered: 1,
            used_vector_search: false,
        };
        let out = plan.display();
        // Should have the name but no location line (no path)
        assert!(out.contains("[symbol] X"));
        // The line after "[symbol] X (score: 0.100)" should NOT be an indented path
        // Just verify no "      /" or "      src/" pattern
    }

    #[test]
    fn display_item_empty_summary_not_shown() {
        let item = ReadingPlanItem {
            entity_id: "e::Y".to_string(),
            name: "Y".to_string(),
            kind: "component".to_string(),
            path: Some("lib.rs".to_string()),
            line_start: None,
            line_end: None,
            short_summary: String::new(),
            score: 0.5,
            reasons: vec!["exported".to_string()],
            is_context: false,
            context_via: None,
        };
        let plan = ReadingPlan {
            query: "q".to_string(),
            category: "general".to_string(),
            items: vec![item],
            candidates_considered: 1,
            used_vector_search: false,
        };
        let out = plan.display();
        // The empty summary line should not appear
        let lines: Vec<&str> = out.lines().collect();
        // After "lib.rs" line, the next should be "reasons:" not an empty summary
        let lib_line = lines.iter().position(|l| l.contains("lib.rs")).unwrap();
        assert!(lines[lib_line + 1].contains("reasons:"));
    }
}
