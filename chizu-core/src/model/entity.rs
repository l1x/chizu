use super::edge_kind::EdgeKind;
use super::entity_kind::EntityKind;
use super::id::ComponentId;

/// Source-level visibility qualifier, normalized across languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Visible everywhere (Rust `pub`, Java/TS `public`, Go exported).
    Public,
    /// Visible only within the declaring scope.
    Private,
    /// Visible to subclasses (Java/TS `protected`).
    Protected,
    /// Visible within the component but not exported
    /// (Rust `pub(crate)`, Java package-private, Kotlin `internal`).
    Internal,
}

impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
            Visibility::Protected => "protected",
            Visibility::Internal => "internal",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for Visibility {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" | "pub" => Ok(Visibility::Public),
            "private" => Ok(Visibility::Private),
            "protected" => Ok(Visibility::Protected),
            "internal" | "pub(crate)" => Ok(Visibility::Internal),
            _ => Err(format!("unknown visibility: {s}")),
        }
    }
}

impl rusqlite::types::FromSql for Visibility {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map_err(|e: String| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

impl rusqlite::types::ToSql for Visibility {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.to_string().into())
    }
}

/// Category of a file record in the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Source,
    Doc,
    Config,
    Build,
    Binary,
    Data,
    Template,
    Migration,
    Workflow,
    Other,
}

impl std::fmt::Display for FileKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            FileKind::Source => "source",
            FileKind::Doc => "doc",
            FileKind::Config => "config",
            FileKind::Build => "build",
            FileKind::Binary => "binary",
            FileKind::Data => "data",
            FileKind::Template => "template",
            FileKind::Migration => "migration",
            FileKind::Workflow => "workflow",
            FileKind::Other => "other",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for FileKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "source" => Ok(FileKind::Source),
            "doc" => Ok(FileKind::Doc),
            "config" => Ok(FileKind::Config),
            "build" => Ok(FileKind::Build),
            "binary" => Ok(FileKind::Binary),
            "data" => Ok(FileKind::Data),
            "template" => Ok(FileKind::Template),
            "migration" => Ok(FileKind::Migration),
            "workflow" => Ok(FileKind::Workflow),
            "other" => Ok(FileKind::Other),
            _ => Err(format!("unknown file kind: {s}")),
        }
    }
}

impl rusqlite::types::FromSql for FileKind {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map_err(|e: String| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

impl rusqlite::types::ToSql for FileKind {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.to_string().into())
    }
}

/// An entity in the knowledge graph.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entity {
    pub id: String,
    pub kind: EntityKind,
    pub name: String,
    pub component_id: Option<ComponentId>,
    pub path: Option<String>,
    pub language: Option<String>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub visibility: Option<Visibility>,
    pub exported: bool,
}

impl Entity {
    pub fn new(id: impl Into<String>, kind: EntityKind, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            name: name.into(),
            component_id: None,
            path: None,
            language: None,
            line_start: None,
            line_end: None,
            visibility: None,
            exported: false,
        }
    }

    pub fn with_component(mut self, component_id: ComponentId) -> Self {
        self.component_id = Some(component_id);
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn with_lines(mut self, start: u32, end: u32) -> Self {
        self.line_start = Some(start);
        self.line_end = Some(end);
        self
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = Some(visibility);
        self
    }

    pub fn with_exported(mut self, exported: bool) -> Self {
        self.exported = exported;
        self
    }
}

/// An edge (relationship) between two entities.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    pub src_id: String,
    pub rel: EdgeKind,
    pub dst_id: String,
    pub provenance_path: Option<String>,
    pub provenance_line: Option<u32>,
}

impl Edge {
    pub fn new(
        src_id: impl Into<String>,
        rel: EdgeKind,
        dst_id: impl Into<String>,
    ) -> Self {
        Self {
            src_id: src_id.into(),
            rel,
            dst_id: dst_id.into(),
            provenance_path: None,
            provenance_line: None,
        }
    }

    pub fn with_provenance(mut self, path: impl Into<String>, line: u32) -> Self {
        self.provenance_path = Some(path.into());
        self.provenance_line = Some(line);
        self
    }
}

/// A file record in the database.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub component_id: Option<ComponentId>,
    pub kind: FileKind,
    /// BLAKE3 hash of file content.
    pub hash: String,
    pub indexed: bool,
    pub ignore_reason: Option<String>,
}

impl FileRecord {
    pub fn new(path: impl Into<String>, kind: FileKind, hash: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            component_id: None,
            kind,
            hash: hash.into(),
            indexed: true,
            ignore_reason: None,
        }
    }

    pub fn ignored(mut self, reason: impl Into<String>) -> Self {
        self.indexed = false;
        self.ignore_reason = Some(reason.into());
        self
    }

    pub fn with_component(mut self, component_id: ComponentId) -> Self {
        self.component_id = Some(component_id);
        self
    }
}

/// A summary for an entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub entity_id: String,
    pub short_summary: String,
    pub detailed_summary: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub updated_at: String,
    /// Hash of the source content (for invalidation).
    pub source_hash: Option<String>,
}

impl Summary {
    pub fn new(entity_id: impl Into<String>, short_summary: impl Into<String>) -> Self {
        Self {
            entity_id: entity_id.into(),
            short_summary: short_summary.into(),
            detailed_summary: None,
            keywords: None,
            updated_at: now_rfc3339(),
            source_hash: None,
        }
    }

    pub fn with_detailed(mut self, detailed: impl Into<String>) -> Self {
        self.detailed_summary = Some(detailed.into());
        self
    }

    pub fn with_keywords(mut self, keywords: &[&str]) -> Self {
        self.keywords = Some(keywords.iter().map(|&s| s.to_string()).collect());
        self
    }

    pub fn with_source_hash(mut self, hash: impl Into<String>) -> Self {
        self.source_hash = Some(hash.into());
        self
    }
}

/// A task route assignment for an entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskRoute {
    pub task_name: String,
    pub entity_id: String,
    /// Higher = more relevant.
    pub priority: i32,
}

impl TaskRoute {
    pub fn new(task_name: impl Into<String>, entity_id: impl Into<String>, priority: i32) -> Self {
        Self {
            task_name: task_name.into(),
            entity_id: entity_id.into(),
            priority,
        }
    }
}

/// Metadata for an embedding (vectors live in usearch, not SQLite).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EmbeddingMeta {
    pub entity_id: String,
    pub model: String,
    pub dimensions: u32,
    pub updated_at: String,
    /// blake3 hash of entity_id truncated to i64.
    pub usearch_key: Option<i64>,
}

impl EmbeddingMeta {
    pub fn new(entity_id: impl Into<String>, model: impl Into<String>, dimensions: u32) -> Self {
        Self {
            entity_id: entity_id.into(),
            model: model.into(),
            dimensions,
            updated_at: now_rfc3339(),
            usearch_key: None,
        }
    }

    pub fn with_usearch_key(mut self, key: i64) -> Self {
        self.usearch_key = Some(key);
        self
    }
}

fn now_rfc3339() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}

#[cfg(test)]
mod tests {
    use super::EdgeKind;
    use super::super::entity_kind::EntityKind;
    use super::*;

    #[test]
    fn test_entity_builder() {
        let entity = Entity::new("symbol::src/main.rs::main", EntityKind::Symbol, "main")
            .with_component(ComponentId::new("cargo", "."))
            .with_path("src/main.rs")
            .with_language("rust")
            .with_lines(1, 10)
            .with_visibility(Visibility::Public)
            .with_exported(true);

        assert_eq!(entity.id, "symbol::src/main.rs::main");
        assert_eq!(entity.kind, EntityKind::Symbol);
        assert_eq!(entity.name, "main");
        assert!(entity.component_id.is_some());
        assert_eq!(entity.path, Some("src/main.rs".to_string()));
        assert_eq!(entity.visibility, Some(Visibility::Public));
        assert!(entity.exported);
    }

    #[test]
    fn test_edge_builder() {
        let edge = Edge::new(
            "source_unit::src/main.rs",
            EdgeKind::Defines,
            "symbol::src/main.rs::main",
        )
        .with_provenance("src/main.rs", 5);

        assert_eq!(edge.src_id, "source_unit::src/main.rs");
        assert_eq!(edge.dst_id, "symbol::src/main.rs::main");
        assert_eq!(edge.provenance_line, Some(5));
    }

    #[test]
    fn test_file_record() {
        let file = FileRecord::new("src/lib.rs", FileKind::Source, "abc123")
            .with_component(ComponentId::new("cargo", "."));

        assert_eq!(file.path, "src/lib.rs");
        assert_eq!(file.kind, FileKind::Source);
        assert!(file.indexed);
    }

    #[test]
    fn test_file_record_ignored() {
        let file =
            FileRecord::new("target/debug/foo", FileKind::Binary, "").ignored("build artifact");

        assert!(!file.indexed);
        assert_eq!(file.ignore_reason, Some("build artifact".to_string()));
    }

    #[test]
    fn test_task_route() {
        let route = TaskRoute::new("debug", "symbol::src/main.rs::main", 80);
        assert_eq!(route.task_name, "debug");
        assert_eq!(route.priority, 80);
    }

    #[test]
    fn test_embedding_meta() {
        let meta =
            EmbeddingMeta::new("symbol::main", "nomic-embed-text", 768).with_usearch_key(12345);

        assert_eq!(meta.entity_id, "symbol::main");
        assert_eq!(meta.dimensions, 768);
        assert_eq!(meta.usearch_key, Some(12345));
    }

    #[test]
    fn test_entity_roundtrip() {
        let entity = Entity::new("test::1", EntityKind::Symbol, "test").with_exported(true);

        let json = serde_json::to_string(&entity).unwrap();
        let deserialized: Entity = serde_json::from_str(&json).unwrap();
        assert_eq!(entity.id, deserialized.id);
        assert_eq!(entity.kind, deserialized.kind);
    }

    #[test]
    fn test_edge_roundtrip() {
        let edge = Edge::new("a", EdgeKind::Defines, "b");
        let json = serde_json::to_string(&edge).unwrap();
        let deserialized: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(edge.src_id, deserialized.src_id);
        assert_eq!(edge.rel, deserialized.rel);
    }

    #[test]
    fn test_visibility_roundtrip() {
        for vis in [
            Visibility::Public,
            Visibility::Private,
            Visibility::Protected,
            Visibility::Internal,
        ] {
            let s = vis.to_string();
            let parsed: Visibility = s.parse().unwrap();
            assert_eq!(vis, parsed);
        }
    }

    #[test]
    fn test_visibility_from_rust_syntax() {
        assert_eq!("pub".parse::<Visibility>().unwrap(), Visibility::Public);
        assert_eq!(
            "pub(crate)".parse::<Visibility>().unwrap(),
            Visibility::Internal
        );
    }

    #[test]
    fn test_file_kind_roundtrip() {
        for kind in [
            FileKind::Source,
            FileKind::Doc,
            FileKind::Config,
            FileKind::Build,
            FileKind::Binary,
            FileKind::Data,
            FileKind::Template,
            FileKind::Migration,
            FileKind::Workflow,
            FileKind::Other,
        ] {
            let s = kind.to_string();
            let parsed: FileKind = s.parse().unwrap();
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn test_summary_keywords() {
        let summary = Summary::new("e1", "short").with_keywords(&["foo", "bar"]);
        assert_eq!(
            summary.keywords,
            Some(vec!["foo".to_string(), "bar".to_string()])
        );
    }
}
