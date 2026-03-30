/// The kind of an entity in the knowledge graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// Top-level repository root
    Repo,
    /// Filesystem directory in the project tree
    Directory,
    /// Build-defined unit: Cargo crate, npm package
    Component,
    /// Individual source file
    SourceUnit,
    /// Function, struct, enum, trait, impl, const, macro, class, interface
    Symbol,
    /// Documentation file (README, PRD, design doc, changelog)
    Doc,
    /// Test function
    Test,
    /// Benchmark function
    Bench,
    /// Build/dev task from mise.toml or similar
    Task,
    /// Cargo feature flag
    Feature,
    /// Dockerfile or docker-compose definition
    Containerized,
    /// Terraform root (directory containing main.tf)
    InfraRoot,
    /// Ansible playbook or similar automation command
    Command,
    /// Markdown with frontmatter in content directories
    ContentPage,
    /// HTML/Astro template file
    Template,
    /// Site root (Astro, Hugo, etc.)
    Site,
    /// SQL migration file
    Migration,
    /// TLA+ specification
    Spec,
    /// CI/CD workflow definition
    Workflow,
    /// Agent configuration file (CLAUDE.md, AGENTS.md)
    AgentConfig,
}

impl std::fmt::Display for EntityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EntityKind::Repo => "repo",
            EntityKind::Directory => "directory",
            EntityKind::Component => "component",
            EntityKind::SourceUnit => "source_unit",
            EntityKind::Symbol => "symbol",
            EntityKind::Doc => "doc",
            EntityKind::Test => "test",
            EntityKind::Bench => "bench",
            EntityKind::Task => "task",
            EntityKind::Feature => "feature",
            EntityKind::Containerized => "containerized",
            EntityKind::InfraRoot => "infra_root",
            EntityKind::Command => "command",
            EntityKind::ContentPage => "content_page",
            EntityKind::Template => "template",
            EntityKind::Site => "site",
            EntityKind::Migration => "migration",
            EntityKind::Spec => "spec",
            EntityKind::Workflow => "workflow",
            EntityKind::AgentConfig => "agent_config",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for EntityKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "repo" => Ok(EntityKind::Repo),
            "directory" => Ok(EntityKind::Directory),
            "component" => Ok(EntityKind::Component),
            "source_unit" => Ok(EntityKind::SourceUnit),
            "symbol" => Ok(EntityKind::Symbol),
            "doc" => Ok(EntityKind::Doc),
            "test" => Ok(EntityKind::Test),
            "bench" => Ok(EntityKind::Bench),
            "task" => Ok(EntityKind::Task),
            "feature" => Ok(EntityKind::Feature),
            "containerized" => Ok(EntityKind::Containerized),
            "infra_root" => Ok(EntityKind::InfraRoot),
            "command" => Ok(EntityKind::Command),
            "content_page" => Ok(EntityKind::ContentPage),
            "template" => Ok(EntityKind::Template),
            "site" => Ok(EntityKind::Site),
            "migration" => Ok(EntityKind::Migration),
            "spec" => Ok(EntityKind::Spec),
            "workflow" => Ok(EntityKind::Workflow),
            "agent_config" => Ok(EntityKind::AgentConfig),
            _ => Err(format!("unknown entity kind: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_kind_roundtrip() {
        let kinds = [
            EntityKind::Repo,
            EntityKind::Directory,
            EntityKind::Component,
            EntityKind::SourceUnit,
            EntityKind::Symbol,
            EntityKind::Doc,
            EntityKind::Test,
            EntityKind::Bench,
            EntityKind::Task,
            EntityKind::Feature,
            EntityKind::Containerized,
            EntityKind::InfraRoot,
            EntityKind::Command,
            EntityKind::ContentPage,
            EntityKind::Template,
            EntityKind::Site,
            EntityKind::Migration,
            EntityKind::Spec,
            EntityKind::Workflow,
            EntityKind::AgentConfig,
        ];

        for kind in &kinds {
            let serialized = serde_json::to_string(kind).unwrap();
            let deserialized: EntityKind = serde_json::from_str(&serialized).unwrap();
            assert_eq!(*kind, deserialized);

            let str_repr = kind.to_string();
            let parsed: EntityKind = str_repr.parse().unwrap();
            assert_eq!(*kind, parsed);
        }
    }

    #[test]
    fn test_entity_kind_display() {
        assert_eq!(EntityKind::Symbol.to_string(), "symbol");
        assert_eq!(EntityKind::Component.to_string(), "component");
    }

    #[test]
    fn test_entity_kind_from_str() {
        assert_eq!("test".parse::<EntityKind>().unwrap(), EntityKind::Test);
        assert!("unknown".parse::<EntityKind>().is_err());
    }
}
