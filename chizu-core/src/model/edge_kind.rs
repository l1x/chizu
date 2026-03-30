/// The kind of relationship (edge) between entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Component -> SourceUnit, Repo -> Component, Site -> ContentPage
    Contains,
    /// SourceUnit -> Symbol
    Defines,
    /// Component -> Component
    DependsOn,
    /// SourceUnit -> Symbol
    Reexports,
    /// Component -> Doc, Site -> Doc
    DocumentedBy,
    /// SourceUnit -> Test
    TestedBy,
    /// SourceUnit -> Bench
    BenchmarkedBy,
    /// Any -> Any
    RelatedTo,
    /// Component -> Feature, Repo -> AgentConfig
    ConfiguredBy,
    /// Task -> Containerized
    Builds,
    /// Site -> InfraRoot
    Deploys,
    /// Impl -> Trait
    Implements,
    /// Repo -> Task
    OwnsTask,
    /// Component -> Feature
    DeclaresFeature,
    /// Feature -> Feature
    FeatureEnables,
    /// Repo -> Migration
    Migrates,
    /// Repo -> Spec
    Specifies,
    /// Template -> ContentPage, Template -> Site
    Renders,
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EdgeKind::Contains => "contains",
            EdgeKind::Defines => "defines",
            EdgeKind::DependsOn => "depends_on",
            EdgeKind::Reexports => "reexports",
            EdgeKind::DocumentedBy => "documented_by",
            EdgeKind::TestedBy => "tested_by",
            EdgeKind::BenchmarkedBy => "benchmarked_by",
            EdgeKind::RelatedTo => "related_to",
            EdgeKind::ConfiguredBy => "configured_by",
            EdgeKind::Builds => "builds",
            EdgeKind::Deploys => "deploys",
            EdgeKind::Implements => "implements",
            EdgeKind::OwnsTask => "owns_task",
            EdgeKind::DeclaresFeature => "declares_feature",
            EdgeKind::FeatureEnables => "feature_enables",
            EdgeKind::Migrates => "migrates",
            EdgeKind::Specifies => "specifies",
            EdgeKind::Renders => "renders",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for EdgeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "contains" => Ok(EdgeKind::Contains),
            "defines" => Ok(EdgeKind::Defines),
            "depends_on" => Ok(EdgeKind::DependsOn),
            "reexports" => Ok(EdgeKind::Reexports),
            "documented_by" => Ok(EdgeKind::DocumentedBy),
            "tested_by" => Ok(EdgeKind::TestedBy),
            "benchmarked_by" => Ok(EdgeKind::BenchmarkedBy),
            "related_to" => Ok(EdgeKind::RelatedTo),
            "configured_by" => Ok(EdgeKind::ConfiguredBy),
            "builds" => Ok(EdgeKind::Builds),
            "deploys" => Ok(EdgeKind::Deploys),
            "implements" => Ok(EdgeKind::Implements),
            "owns_task" => Ok(EdgeKind::OwnsTask),
            "declares_feature" => Ok(EdgeKind::DeclaresFeature),
            "feature_enables" => Ok(EdgeKind::FeatureEnables),
            "migrates" => Ok(EdgeKind::Migrates),
            "specifies" => Ok(EdgeKind::Specifies),
            "renders" => Ok(EdgeKind::Renders),
            _ => Err(format!("unknown edge kind: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_kind_roundtrip() {
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
            EdgeKind::Migrates,
            EdgeKind::Specifies,
            EdgeKind::Renders,
        ];

        for kind in &kinds {
            let serialized = serde_json::to_string(kind).unwrap();
            let deserialized: EdgeKind = serde_json::from_str(&serialized).unwrap();
            assert_eq!(*kind, deserialized);

            let str_repr = kind.to_string();
            let parsed: EdgeKind = str_repr.parse().unwrap();
            assert_eq!(*kind, parsed);
        }
    }

    #[test]
    fn test_edge_kind_display() {
        assert_eq!(EdgeKind::Defines.to_string(), "defines");
        assert_eq!(EdgeKind::DependsOn.to_string(), "depends_on");
    }

    #[test]
    fn test_edge_kind_from_str() {
        assert_eq!("contains".parse::<EdgeKind>().unwrap(), EdgeKind::Contains);
        assert!("unknown".parse::<EdgeKind>().is_err());
    }
}
