/// Task category for query classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    Understand,
    Debug,
    Build,
    Test,
    Deploy,
    Configure,
    General,
}

impl std::str::FromStr for TaskCategory {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "understand" => Ok(Self::Understand),
            "debug" => Ok(Self::Debug),
            "build" => Ok(Self::Build),
            "test" => Ok(Self::Test),
            "deploy" => Ok(Self::Deploy),
            "configure" => Ok(Self::Configure),
            "general" => Ok(Self::General),
            _ => Err(format!(
                "unknown category '{s}': expected understand|debug|build|test|deploy|configure|general"
            )),
        }
    }
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TaskCategory::Understand => "understand",
            TaskCategory::Debug => "debug",
            TaskCategory::Build => "build",
            TaskCategory::Test => "test",
            TaskCategory::Deploy => "deploy",
            TaskCategory::Configure => "configure",
            TaskCategory::General => "general",
        };
        f.write_str(s)
    }
}

impl TaskCategory {
    /// Returns the route names associated with this category for task route lookup.
    pub fn route_names(&self) -> &[&str] {
        match self {
            TaskCategory::Understand => &["understand", "architecture"],
            TaskCategory::Debug => &["debug", "fix"],
            TaskCategory::Build => &["build", "implement"],
            TaskCategory::Test => &["test", "bench"],
            TaskCategory::Deploy => &["deploy", "release"],
            TaskCategory::Configure => &["configure", "setup"],
            TaskCategory::General => &[],
        }
    }

    /// Returns the preferred entity kinds for this category.
    pub fn preferred_kinds(&self) -> &[&str] {
        match self {
            TaskCategory::Understand => &[
                "component",
                "source_unit",
                "doc",
                "symbol",
                "content_page",
                "agent_config",
            ],
            TaskCategory::Debug => &["source_unit", "symbol", "test", "spec"],
            TaskCategory::Build => &[
                "component",
                "source_unit",
                "symbol",
                "feature",
                "template",
                "migration",
            ],
            TaskCategory::Test => &["test", "bench", "source_unit", "spec"],
            TaskCategory::Deploy => &["containerized", "infra_root", "task", "command", "site"],
            TaskCategory::Configure => &[
                "component",
                "feature",
                "infra_root",
                "agent_config",
                "workflow",
            ],
            TaskCategory::General => &["component", "source_unit", "symbol"],
        }
    }
}

/// Classify a query into a task category using heuristic keyword matching.
pub fn classify_query(query: &str) -> TaskCategory {
    use TaskCategory::*;

    // Fixed-size scores array indexed by category ordinal.
    const CATEGORIES: [TaskCategory; 6] = [Debug, Build, Test, Deploy, Configure, Understand];
    let mut scores = [0u32; 6];

    let lower = query.to_lowercase();
    for token in lower.split_whitespace() {
        if matches!(
            token,
            "debug" | "fix" | "bug" | "error" | "panic" | "crash" | "trace"
        ) {
            scores[0] += 1;
        }
        if matches!(token, "build" | "implement" | "add" | "create" | "write") {
            scores[1] += 1;
        }
        if matches!(token, "test" | "tests" | "bench" | "verify" | "assert") {
            scores[2] += 1;
        }
        if matches!(token, "deploy" | "release" | "publish" | "ship" | "prod") {
            scores[3] += 1;
        }
        if matches!(token, "configure" | "setup" | "settings" | "env" | "config") {
            scores[4] += 1;
        }
        if matches!(
            token,
            "understand" | "how" | "what" | "architecture" | "design" | "overview"
        ) {
            scores[5] += 1;
        }
    }

    let (best_idx, &best_score) = scores.iter().enumerate().max_by_key(|(_, s)| *s).unwrap();
    if best_score == 0 {
        General
    } else {
        CATEGORIES[best_idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_debug() {
        assert_eq!(classify_query("fix the auth bug"), TaskCategory::Debug);
        assert_eq!(classify_query("debug panic in router"), TaskCategory::Debug);
    }

    #[test]
    fn test_classify_deploy() {
        assert_eq!(classify_query("deploy to prod"), TaskCategory::Deploy);
    }

    #[test]
    fn test_classify_understand() {
        assert_eq!(
            classify_query("how does routing work"),
            TaskCategory::Understand
        );
        assert_eq!(
            classify_query("architecture overview"),
            TaskCategory::Understand
        );
    }

    #[test]
    fn test_classify_build() {
        assert_eq!(classify_query("implement new feature"), TaskCategory::Build);
    }

    #[test]
    fn test_classify_test() {
        assert_eq!(classify_query("run tests for auth"), TaskCategory::Test);
    }

    #[test]
    fn test_classify_configure() {
        assert_eq!(classify_query("setup env config"), TaskCategory::Configure);
    }

    #[test]
    fn test_classify_general_fallback() {
        assert_eq!(classify_query("foo bar baz"), TaskCategory::General);
    }

    #[test]
    fn test_task_category_roundtrip() {
        for cat in [
            TaskCategory::Understand,
            TaskCategory::Debug,
            TaskCategory::Build,
            TaskCategory::Test,
            TaskCategory::Deploy,
            TaskCategory::Configure,
            TaskCategory::General,
        ] {
            let s = cat.to_string();
            let parsed: TaskCategory = s.parse().unwrap();
            assert_eq!(cat, parsed);
        }
    }
}
