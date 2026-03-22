use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Repo,
    Component,
    SourceUnit,
    Symbol,
    Doc,
    Test,
    Bench,
    Task,
    Deployable,
    InfraRoot,
    Command,
    Feature,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Component => "component",
            Self::SourceUnit => "source_unit",
            Self::Symbol => "symbol",
            Self::Doc => "doc",
            Self::Test => "test",
            Self::Bench => "bench",
            Self::Task => "task",
            Self::Deployable => "deployable",
            Self::InfraRoot => "infra_root",
            Self::Command => "command",
            Self::Feature => "feature",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "repo" => Some(Self::Repo),
            "component" => Some(Self::Component),
            "source_unit" => Some(Self::SourceUnit),
            "symbol" => Some(Self::Symbol),
            "doc" => Some(Self::Doc),
            "test" => Some(Self::Test),
            "bench" => Some(Self::Bench),
            "task" => Some(Self::Task),
            "deployable" => Some(Self::Deployable),
            "infra_root" => Some(Self::InfraRoot),
            "command" => Some(Self::Command),
            "feature" => Some(Self::Feature),
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
            EntityKind::Component,
            EntityKind::SourceUnit,
            EntityKind::Symbol,
            EntityKind::Doc,
            EntityKind::Test,
            EntityKind::Bench,
            EntityKind::Task,
            EntityKind::Deployable,
            EntityKind::InfraRoot,
            EntityKind::Command,
            EntityKind::Feature,
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
