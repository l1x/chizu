use super::entity_kind::EntityKind;
use super::id::ComponentId;

/// An entity in the knowledge graph.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entity {
    /// Unique identifier for this entity.
    pub id: String,
    /// The kind of entity.
    pub kind: EntityKind,
    /// Human-readable name.
    pub name: String,
    /// The component this entity belongs to (if any).
    pub component_id: Option<ComponentId>,
    /// File path where this entity is defined (if file-backed).
    pub path: Option<String>,
    /// Programming language (for source entities).
    pub language: Option<String>,
    /// Starting line number (1-indexed).
    pub line_start: Option<u32>,
    /// Ending line number (1-indexed).
    pub line_end: Option<u32>,
    /// Visibility (public, private, etc.)
    pub visibility: Option<String>,
    /// Whether this is an exported/public symbol.
    pub exported: bool,
}

impl Entity {
    /// Create a new entity with required fields.
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

    /// Set the component ID.
    pub fn with_component(mut self, component_id: ComponentId) -> Self {
        self.component_id = Some(component_id);
        self
    }

    /// Set the file path.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the language.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Set the line range.
    pub fn with_lines(mut self, start: u32, end: u32) -> Self {
        self.line_start = Some(start);
        self.line_end = Some(end);
        self
    }

    /// Set visibility.
    pub fn with_visibility(mut self, visibility: impl Into<String>) -> Self {
        self.visibility = Some(visibility.into());
        self
    }

    /// Set exported flag.
    pub fn with_exported(mut self, exported: bool) -> Self {
        self.exported = exported;
        self
    }
}

/// An edge (relationship) between two entities.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Edge {
    /// Source entity ID.
    pub src_id: String,
    /// Relationship kind.
    pub rel: super::edge_kind::EdgeKind,
    /// Destination entity ID.
    pub dst_id: String,
    /// File path where this relationship is defined (if any).
    pub provenance_path: Option<String>,
    /// Line number where this relationship is defined (if any).
    pub provenance_line: Option<u32>,
}

impl Edge {
    /// Create a new edge.
    pub fn new(
        src_id: impl Into<String>,
        rel: super::edge_kind::EdgeKind,
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

    /// Set provenance information.
    pub fn with_provenance(mut self, path: impl Into<String>, line: u32) -> Self {
        self.provenance_path = Some(path.into());
        self.provenance_line = Some(line);
        self
    }
}

/// A file record in the database.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileRecord {
    /// Repository-relative path.
    pub path: String,
    /// Component ID this file belongs to.
    pub component_id: Option<ComponentId>,
    /// Kind of file (source, doc, config, etc.)
    pub kind: String,
    /// BLAKE3 hash of file content.
    pub hash: String,
    /// Whether this file is indexed.
    pub indexed: bool,
    /// Reason for ignoring (if not indexed).
    pub ignore_reason: Option<String>,
}

impl FileRecord {
    /// Create a new file record.
    pub fn new(path: impl Into<String>, kind: impl Into<String>, hash: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            component_id: None,
            kind: kind.into(),
            hash: hash.into(),
            indexed: true,
            ignore_reason: None,
        }
    }

    /// Mark as ignored with a reason.
    pub fn ignored(mut self, reason: impl Into<String>) -> Self {
        self.indexed = false;
        self.ignore_reason = Some(reason.into());
        self
    }

    /// Set the component ID.
    pub fn with_component(mut self, component_id: ComponentId) -> Self {
        self.component_id = Some(component_id);
        self
    }
}

/// A summary for an entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    /// Entity ID this summary is for.
    pub entity_id: String,
    /// Short summary (one sentence).
    pub short_summary: String,
    /// Detailed summary (paragraph).
    pub detailed_summary: Option<String>,
    /// Keywords extracted from the entity.
    pub keywords_json: Option<String>,
    /// When this summary was generated.
    pub updated_at: String,
    /// Hash of the source content (for invalidation).
    pub source_hash: Option<String>,
}

impl Summary {
    /// Create a new summary.
    pub fn new(entity_id: impl Into<String>, short_summary: impl Into<String>) -> Self {
        Self {
            entity_id: entity_id.into(),
            short_summary: short_summary.into(),
            detailed_summary: None,
            keywords_json: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
            source_hash: None,
        }
    }

    /// Set detailed summary.
    pub fn with_detailed(mut self, detailed: impl Into<String>) -> Self {
        self.detailed_summary = Some(detailed.into());
        self
    }

    /// Set keywords.
    pub fn with_keywords(mut self, keywords: &[&str]) -> Self {
        self.keywords_json = Some(serde_json::to_string(keywords).unwrap_or_default());
        self
    }

    /// Set source hash.
    pub fn with_source_hash(mut self, hash: impl Into<String>) -> Self {
        self.source_hash = Some(hash.into());
        self
    }
}

/// A task route assignment for an entity.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskRoute {
    /// Task name (e.g., "debug", "build", "test").
    pub task_name: String,
    /// Entity ID.
    pub entity_id: String,
    /// Priority score (higher = more relevant).
    pub priority: i32,
}

impl TaskRoute {
    /// Create a new task route.
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
    /// Entity ID this embedding is for.
    pub entity_id: String,
    /// Model used to generate the embedding.
    pub model: String,
    /// Number of dimensions.
    pub dimensions: u32,
    /// When this embedding was generated.
    pub updated_at: String,
    /// usearch key (blake3 hash truncated to i64).
    pub usearch_key: Option<i64>,
}

impl EmbeddingMeta {
    /// Create new embedding metadata.
    pub fn new(entity_id: impl Into<String>, model: impl Into<String>, dimensions: u32) -> Self {
        Self {
            entity_id: entity_id.into(),
            model: model.into(),
            dimensions,
            updated_at: chrono::Utc::now().to_rfc3339(),
            usearch_key: None,
        }
    }

    /// Set the usearch key.
    pub fn with_usearch_key(mut self, key: i64) -> Self {
        self.usearch_key = Some(key);
        self
    }
}

// Need chrono for timestamps - using a simple fallback for now
mod chrono {
    pub struct Utc;
    impl Utc {
        pub fn now() -> Self {
            Self
        }
        pub fn to_rfc3339(&self) -> String {
            // Simple RFC3339 format - in production use actual chrono crate
            "2024-01-01T00:00:00Z".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::edge_kind::EdgeKind;
    use super::super::entity_kind::EntityKind;
    use super::*;

    #[test]
    fn test_entity_builder() {
        let entity = Entity::new("symbol::src/main.rs::main", EntityKind::Symbol, "main")
            .with_component(ComponentId::new("cargo", "."))
            .with_path("src/main.rs")
            .with_language("rust")
            .with_lines(1, 10)
            .with_visibility("pub")
            .with_exported(true);

        assert_eq!(entity.id, "symbol::src/main.rs::main");
        assert_eq!(entity.kind, EntityKind::Symbol);
        assert_eq!(entity.name, "main");
        assert!(entity.component_id.is_some());
        assert_eq!(entity.path, Some("src/main.rs".to_string()));
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
        let file = FileRecord::new("src/lib.rs", "rust", "abc123")
            .with_component(ComponentId::new("cargo", "."));

        assert_eq!(file.path, "src/lib.rs");
        assert!(file.indexed);
    }

    #[test]
    fn test_file_record_ignored() {
        let file = FileRecord::new("target/debug/foo", "binary", "").ignored("build artifact");

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
}
