use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    Defines,
    DependsOn,
    Reexports,
    DocumentedBy,
    TestedBy,
    BenchmarkedBy,
    RelatedTo,
    ConfiguredBy,
    Builds,
    Deploys,
    Implements,
    OwnsTask,
    DeclaresFeature,
    FeatureEnables,
    Mentions,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Defines => "defines",
            Self::DependsOn => "depends_on",
            Self::Reexports => "reexports",
            Self::DocumentedBy => "documented_by",
            Self::TestedBy => "tested_by",
            Self::BenchmarkedBy => "benchmarked_by",
            Self::RelatedTo => "related_to",
            Self::ConfiguredBy => "configured_by",
            Self::Builds => "builds",
            Self::Deploys => "deploys",
            Self::Implements => "implements",
            Self::OwnsTask => "owns_task",
            Self::DeclaresFeature => "declares_feature",
            Self::FeatureEnables => "feature_enables",
            Self::Mentions => "mentions",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "contains" => Some(Self::Contains),
            "defines" => Some(Self::Defines),
            "depends_on" => Some(Self::DependsOn),
            "reexports" => Some(Self::Reexports),
            "documented_by" => Some(Self::DocumentedBy),
            "tested_by" => Some(Self::TestedBy),
            "benchmarked_by" => Some(Self::BenchmarkedBy),
            "related_to" => Some(Self::RelatedTo),
            "configured_by" => Some(Self::ConfiguredBy),
            "builds" => Some(Self::Builds),
            "deploys" => Some(Self::Deploys),
            "implements" => Some(Self::Implements),
            "owns_task" => Some(Self::OwnsTask),
            "declares_feature" => Some(Self::DeclaresFeature),
            "feature_enables" => Some(Self::FeatureEnables),
            "mentions" => Some(Self::Mentions),
            _ => None,
        }
    }
}

impl fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub src_id: String,
    pub rel: EdgeKind,
    pub dst_id: String,
    pub provenance_path: Option<String>,
    pub provenance_line: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_kind_round_trip() {
        let kinds = [
            EdgeKind::Contains,
            EdgeKind::Defines,
            EdgeKind::DependsOn,
            EdgeKind::Reexports,
            EdgeKind::DocumentedBy,
            EdgeKind::TestedBy,
            EdgeKind::BenchmarkedBy,
            EdgeKind::RelatedTo,
            EdgeKind::ConfiguredBy,
            EdgeKind::Builds,
            EdgeKind::Deploys,
            EdgeKind::Implements,
            EdgeKind::OwnsTask,
            EdgeKind::DeclaresFeature,
            EdgeKind::FeatureEnables,
            EdgeKind::Mentions,
        ];
        for kind in &kinds {
            let s = kind.as_str();
            let parsed = EdgeKind::parse(s).unwrap();
            assert_eq!(*kind, parsed);
        }
    }

    #[test]
    fn edge_kind_parse_invalid() {
        assert!(EdgeKind::parse("nope").is_none());
    }
}
