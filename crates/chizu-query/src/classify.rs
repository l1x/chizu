use chizu_core::model::EntityKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

impl TaskCategory {
    /// Classify a query string by heuristic keyword matching.
    pub fn classify(query: &str) -> Self {
        let lower = query.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();

        let contains = |kw: &str| words.iter().any(|w| w.contains(kw));

        if contains("fix")
            || contains("bug")
            || contains("error")
            || contains("crash")
            || contains("panic")
        {
            TaskCategory::Debug
        } else if contains("test") || contains("coverage") || contains("bench") {
            TaskCategory::Test
        } else if contains("deploy") || contains("release") || contains("ci") || contains("infra") {
            TaskCategory::Deploy
        } else if contains("config") || contains("setup") || contains("environment") {
            TaskCategory::Configure
        } else if contains("add")
            || contains("implement")
            || contains("create")
            || contains("feature")
        {
            TaskCategory::Build
        } else if contains("how")
            || contains("what")
            || contains("explain")
            || contains("architecture")
            || contains("understand")
        {
            TaskCategory::Understand
        } else {
            TaskCategory::General
        }
    }

    /// Task route names associated with this category.
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

    /// Entity kinds that should receive a rerank boost for this category.
    pub fn preferred_kinds(&self) -> &[EntityKind] {
        match self {
            TaskCategory::Understand => &[
                EntityKind::Component,
                EntityKind::SourceUnit,
                EntityKind::Doc,
                EntityKind::Symbol,
                EntityKind::ContentPage,
                EntityKind::AgentConfig,
            ],
            TaskCategory::Debug => &[
                EntityKind::SourceUnit,
                EntityKind::Symbol,
                EntityKind::Test,
                EntityKind::Spec,
            ],
            TaskCategory::Build => &[
                EntityKind::Component,
                EntityKind::SourceUnit,
                EntityKind::Symbol,
                EntityKind::Feature,
                EntityKind::Template,
                EntityKind::Migration,
            ],
            TaskCategory::Test => &[
                EntityKind::Test,
                EntityKind::Bench,
                EntityKind::SourceUnit,
                EntityKind::Spec,
            ],
            TaskCategory::Deploy => &[
                EntityKind::Containerized,
                EntityKind::InfraRoot,
                EntityKind::Task,
                EntityKind::Command,
                EntityKind::Site,
            ],
            TaskCategory::Configure => &[
                EntityKind::Component,
                EntityKind::Feature,
                EntityKind::InfraRoot,
                EntityKind::AgentConfig,
                EntityKind::Workflow,
            ],
            TaskCategory::General => &[
                EntityKind::Component,
                EntityKind::SourceUnit,
                EntityKind::Symbol,
            ],
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TaskCategory::Understand => "understand",
            TaskCategory::Debug => "debug",
            TaskCategory::Build => "build",
            TaskCategory::Test => "test",
            TaskCategory::Deploy => "deploy",
            TaskCategory::Configure => "configure",
            TaskCategory::General => "general",
        }
    }
}

impl std::str::FromStr for TaskCategory {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "understand" => Ok(TaskCategory::Understand),
            "debug" => Ok(TaskCategory::Debug),
            "build" => Ok(TaskCategory::Build),
            "test" => Ok(TaskCategory::Test),
            "deploy" => Ok(TaskCategory::Deploy),
            "configure" => Ok(TaskCategory::Configure),
            "general" => Ok(TaskCategory::General),
            _ => Err(format!(
                "invalid category '{}'. Valid values: understand, debug, build, test, deploy, configure, general",
                s
            )),
        }
    }
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- classify() ---

    #[test]
    fn classify_debug_keywords() {
        assert_eq!(
            TaskCategory::classify("fix the login bug"),
            TaskCategory::Debug
        );
        assert_eq!(
            TaskCategory::classify("there is an error in parsing"),
            TaskCategory::Debug
        );
        assert_eq!(
            TaskCategory::classify("the server keeps crashing"),
            TaskCategory::Debug
        );
        assert_eq!(
            TaskCategory::classify("panic in the handler"),
            TaskCategory::Debug
        );
        assert_eq!(TaskCategory::classify("Fix this"), TaskCategory::Debug);
    }

    #[test]
    fn classify_test_keywords() {
        assert_eq!(TaskCategory::classify("run the tests"), TaskCategory::Test);
        assert_eq!(TaskCategory::classify("check coverage"), TaskCategory::Test);
        assert_eq!(
            TaskCategory::classify("add a bench for hashing"),
            TaskCategory::Test
        );
    }

    #[test]
    fn classify_deploy_keywords() {
        assert_eq!(
            TaskCategory::classify("deploy to production"),
            TaskCategory::Deploy
        );
        assert_eq!(
            TaskCategory::classify("cut a release"),
            TaskCategory::Deploy
        );
        assert_eq!(
            TaskCategory::classify("update ci pipeline"),
            TaskCategory::Deploy
        );
        assert_eq!(
            TaskCategory::classify("change infra config"),
            TaskCategory::Deploy
        );
    }

    #[test]
    fn classify_configure_keywords() {
        assert_eq!(
            TaskCategory::classify("update the config file"),
            TaskCategory::Configure
        );
        assert_eq!(
            TaskCategory::classify("setup the environment"),
            TaskCategory::Configure
        );
        assert_eq!(
            TaskCategory::classify("environment variables"),
            TaskCategory::Configure
        );
    }

    #[test]
    fn classify_build_keywords() {
        assert_eq!(
            TaskCategory::classify("add a new endpoint"),
            TaskCategory::Build
        );
        assert_eq!(
            TaskCategory::classify("implement caching"),
            TaskCategory::Build
        );
        assert_eq!(
            TaskCategory::classify("create a helper function"),
            TaskCategory::Build
        );
        assert_eq!(
            TaskCategory::classify("feature flag support"),
            TaskCategory::Build
        );
    }

    #[test]
    fn classify_understand_keywords() {
        assert_eq!(
            TaskCategory::classify("how does the store work"),
            TaskCategory::Understand
        );
        assert_eq!(
            TaskCategory::classify("what is this module"),
            TaskCategory::Understand
        );
        assert_eq!(
            TaskCategory::classify("explain the pipeline"),
            TaskCategory::Understand
        );
        assert_eq!(
            TaskCategory::classify("system architecture overview"),
            TaskCategory::Understand
        );
        assert_eq!(
            TaskCategory::classify("understand the data flow"),
            TaskCategory::Understand
        );
    }

    #[test]
    fn classify_general_fallback() {
        assert_eq!(TaskCategory::classify("store"), TaskCategory::General);
        assert_eq!(TaskCategory::classify(""), TaskCategory::General);
        assert_eq!(TaskCategory::classify("foobar baz"), TaskCategory::General);
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(TaskCategory::classify("FIX the BUG"), TaskCategory::Debug);
        assert_eq!(
            TaskCategory::classify("HOW does this work"),
            TaskCategory::Understand
        );
        assert_eq!(TaskCategory::classify("DEPLOY now"), TaskCategory::Deploy);
    }

    #[test]
    fn classify_debug_takes_priority_over_build() {
        // "fix" is Debug, "add" is Build — Debug checked first
        assert_eq!(
            TaskCategory::classify("fix and add stuff"),
            TaskCategory::Debug
        );
    }

    #[test]
    fn classify_test_takes_priority_over_understand() {
        // "test" is Test, "how" is Understand — Test checked first
        assert_eq!(
            TaskCategory::classify("how to test this"),
            TaskCategory::Test
        );
    }

    #[test]
    fn classify_substring_matching() {
        // "contains" checks w.contains(kw), so "testing" contains "test"
        assert_eq!(
            TaskCategory::classify("testing the system"),
            TaskCategory::Test
        );
        // "bugfix" contains "bug"
        assert_eq!(
            TaskCategory::classify("apply the bugfix"),
            TaskCategory::Debug
        );
        // "configuration" contains "config"
        assert_eq!(
            TaskCategory::classify("configuration management"),
            TaskCategory::Configure
        );
    }

    // --- route_names() ---

    #[test]
    fn route_names_nonempty_for_specific_categories() {
        assert!(!TaskCategory::Understand.route_names().is_empty());
        assert!(!TaskCategory::Debug.route_names().is_empty());
        assert!(!TaskCategory::Build.route_names().is_empty());
        assert!(!TaskCategory::Test.route_names().is_empty());
        assert!(!TaskCategory::Deploy.route_names().is_empty());
        assert!(!TaskCategory::Configure.route_names().is_empty());
    }

    #[test]
    fn route_names_empty_for_general() {
        assert!(TaskCategory::General.route_names().is_empty());
    }

    #[test]
    fn route_names_contain_expected_values() {
        assert!(TaskCategory::Debug.route_names().contains(&"debug"));
        assert!(TaskCategory::Debug.route_names().contains(&"fix"));
        assert!(TaskCategory::Build.route_names().contains(&"build"));
        assert!(TaskCategory::Test.route_names().contains(&"test"));
    }

    // --- preferred_kinds() ---

    #[test]
    fn preferred_kinds_nonempty_for_all_categories() {
        let all = [
            TaskCategory::Understand,
            TaskCategory::Debug,
            TaskCategory::Build,
            TaskCategory::Test,
            TaskCategory::Deploy,
            TaskCategory::Configure,
            TaskCategory::General,
        ];
        for cat in &all {
            assert!(
                !cat.preferred_kinds().is_empty(),
                "{cat} has empty preferred_kinds"
            );
        }
    }

    #[test]
    fn preferred_kinds_debug_includes_test() {
        assert!(TaskCategory::Debug
            .preferred_kinds()
            .contains(&EntityKind::Test));
    }

    #[test]
    fn preferred_kinds_deploy_includes_deployable() {
        assert!(TaskCategory::Deploy
            .preferred_kinds()
            .contains(&EntityKind::Containerized));
    }

    #[test]
    fn preferred_kinds_deploy_includes_site() {
        assert!(TaskCategory::Deploy
            .preferred_kinds()
            .contains(&EntityKind::Site));
    }

    #[test]
    fn preferred_kinds_configure_includes_agent_config() {
        assert!(TaskCategory::Configure
            .preferred_kinds()
            .contains(&EntityKind::AgentConfig));
    }

    #[test]
    fn preferred_kinds_understand_includes_content_page() {
        assert!(TaskCategory::Understand
            .preferred_kinds()
            .contains(&EntityKind::ContentPage));
    }

    #[test]
    fn preferred_kinds_build_includes_template() {
        assert!(TaskCategory::Build
            .preferred_kinds()
            .contains(&EntityKind::Template));
    }

    // --- as_str() / Display ---

    #[test]
    fn as_str_round_trip_all_variants() {
        let all = [
            (TaskCategory::Understand, "understand"),
            (TaskCategory::Debug, "debug"),
            (TaskCategory::Build, "build"),
            (TaskCategory::Test, "test"),
            (TaskCategory::Deploy, "deploy"),
            (TaskCategory::Configure, "configure"),
            (TaskCategory::General, "general"),
        ];
        for (cat, expected) in &all {
            assert_eq!(cat.as_str(), *expected);
            assert_eq!(cat.to_string(), *expected);
        }
    }

    // --- serde ---

    // --- FromStr ---

    #[test]
    fn from_str_all_variants() {
        let cases = [
            ("understand", TaskCategory::Understand),
            ("debug", TaskCategory::Debug),
            ("build", TaskCategory::Build),
            ("test", TaskCategory::Test),
            ("deploy", TaskCategory::Deploy),
            ("configure", TaskCategory::Configure),
            ("general", TaskCategory::General),
        ];
        for (input, expected) in &cases {
            let parsed: TaskCategory = input.parse().unwrap();
            assert_eq!(parsed, *expected);
        }
    }

    #[test]
    fn from_str_case_insensitive() {
        let parsed: TaskCategory = "DEBUG".parse().unwrap();
        assert_eq!(parsed, TaskCategory::Debug);
        let parsed: TaskCategory = "Deploy".parse().unwrap();
        assert_eq!(parsed, TaskCategory::Deploy);
    }

    #[test]
    fn from_str_invalid() {
        let result = "invalid".parse::<TaskCategory>();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("invalid category"));
    }

    // --- serde ---

    #[test]
    fn serde_round_trip() {
        let cat = TaskCategory::Debug;
        let json = serde_json::to_string(&cat).unwrap();
        assert_eq!(json, "\"debug\"");
        let back: TaskCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }

    #[test]
    fn serde_all_variants() {
        let all = [
            TaskCategory::Understand,
            TaskCategory::Debug,
            TaskCategory::Build,
            TaskCategory::Test,
            TaskCategory::Deploy,
            TaskCategory::Configure,
            TaskCategory::General,
        ];
        for cat in &all {
            let json = serde_json::to_string(cat).unwrap();
            let back: TaskCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(*cat, back);
        }
    }
}
