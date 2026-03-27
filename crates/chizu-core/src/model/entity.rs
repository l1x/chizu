use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Repo,
    Directory,
    Component,
    SourceUnit,
    Symbol,
    Doc,
    Test,
    Bench,
    Task,
    Containerized,
    InfraRoot,
    Command,
    Feature,
    ContentPage,
    Template,
    Site,
    Migration,
    Spec,
    Workflow,
    AgentConfig,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Directory => "directory",
            Self::Component => "component",
            Self::SourceUnit => "source_unit",
            Self::Symbol => "symbol",
            Self::Doc => "doc",
            Self::Test => "test",
            Self::Bench => "bench",
            Self::Task => "task",
            Self::Containerized => "containerized",
            Self::InfraRoot => "infra_root",
            Self::Command => "command",
            Self::Feature => "feature",
            Self::ContentPage => "content_page",
            Self::Template => "template",
            Self::Site => "site",
            Self::Migration => "migration",
            Self::Spec => "spec",
            Self::Workflow => "workflow",
            Self::AgentConfig => "agent_config",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "repo" => Some(Self::Repo),
            "directory" => Some(Self::Directory),
            "component" => Some(Self::Component),
            "source_unit" => Some(Self::SourceUnit),
            "symbol" => Some(Self::Symbol),
            "doc" => Some(Self::Doc),
            "test" => Some(Self::Test),
            "bench" => Some(Self::Bench),
            "task" => Some(Self::Task),
            "containerized" => Some(Self::Containerized),
            "infra_root" => Some(Self::InfraRoot),
            "command" => Some(Self::Command),
            "feature" => Some(Self::Feature),
            "content_page" => Some(Self::ContentPage),
            "template" => Some(Self::Template),
            "site" => Some(Self::Site),
            "migration" => Some(Self::Migration),
            "spec" => Some(Self::Spec),
            "workflow" => Some(Self::Workflow),
            "agent_config" => Some(Self::AgentConfig),
            _ => None,
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    pub component_id: Option<String>,
    pub path: Option<String>,
    pub language: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub visibility: Option<String>,
    pub exported: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_kind_round_trip() {
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
            EntityKind::Containerized,
            EntityKind::InfraRoot,
            EntityKind::Command,
            EntityKind::Feature,
            EntityKind::ContentPage,
            EntityKind::Template,
            EntityKind::Site,
            EntityKind::Migration,
            EntityKind::Spec,
            EntityKind::Workflow,
            EntityKind::AgentConfig,
        ];
        for kind in &kinds {
            let s = kind.as_str();
            let parsed = EntityKind::parse(s).unwrap();
            assert_eq!(*kind, parsed);
        }
    }

    #[test]
    fn entity_kind_parse_invalid() {
        assert!(EntityKind::parse("bogus").is_none());
    }
}
