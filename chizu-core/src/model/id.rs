use std::fmt;

/// A canonical component identifier.
///
/// Format: `component::{ecosystem}::{path}`
/// Examples:
/// - `component::cargo::crates/chizu-core`
/// - `component::npm::packages/web`
/// - `component::npm::.` (root package)
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ComponentId(pub String);

impl ComponentId {
    /// Create a new component ID from ecosystem and path.
    pub fn new(ecosystem: &str, path: &str) -> Self {
        Self(format!("component::{}::{}", ecosystem, path))
    }

    /// Parse a component ID from its string representation.
    pub fn parse(s: &str) -> Option<Self> {
        if s.starts_with("component::") {
            Some(Self(s.to_string()))
        } else {
            None
        }
    }

    /// Get the ecosystem part of the ID.
    pub fn ecosystem(&self) -> Option<&str> {
        self.0.strip_prefix("component::")?.split("::").next()
    }

    /// Get the path part of the ID.
    pub fn path(&self) -> Option<&str> {
        self.0.strip_prefix("component::")?.split("::").nth(1)
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ComponentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<ComponentId> for String {
    fn from(id: ComponentId) -> Self {
        id.0
    }
}

/// Generate an entity ID for a symbol.
pub fn symbol_id(file_path: &str, name: &str) -> String {
    format!("symbol::{}::{}", file_path, name)
}

/// Generate an entity ID for a source unit.
pub fn source_unit_id(path: &str) -> String {
    format!("source_unit::{}", path)
}

/// Generate an entity ID for a component.
pub fn component_id(ecosystem: &str, path: &str) -> String {
    format!("component::{}::{}", ecosystem, path)
}

/// Generate an entity ID for a test.
pub fn test_id(file_path: &str, name: &str) -> String {
    format!("test::{}::{}", file_path, name)
}

/// Generate an entity ID for a doc.
pub fn doc_id(path: &str) -> String {
    format!("doc::{}", path)
}

/// Convert a blake3 hash to an i64 for usearch key mapping.
pub fn hash_to_i64(hash: &blake3::Hash) -> i64 {
    let bytes = hash.as_bytes();
    // Take first 8 bytes and interpret as i64
    i64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

/// Generate a usearch key from an entity ID.
pub fn entity_id_to_usearch_key(entity_id: &str) -> i64 {
    let hash = blake3::hash(entity_id.as_bytes());
    hash_to_i64(&hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_id_new() {
        let id = ComponentId::new("cargo", "crates/core");
        assert_eq!(id.0, "component::cargo::crates/core");
    }

    #[test]
    fn test_component_id_parse() {
        let id = ComponentId::parse("component::npm::packages/web").unwrap();
        assert_eq!(id.0, "component::npm::packages/web");
        assert!(ComponentId::parse("invalid").is_none());
    }

    #[test]
    fn test_component_id_ecosystem() {
        let id = ComponentId::new("cargo", "crates/core");
        assert_eq!(id.ecosystem(), Some("cargo"));
    }

    #[test]
    fn test_component_id_path() {
        let id = ComponentId::new("cargo", "crates/core");
        assert_eq!(id.path(), Some("crates/core"));
    }

    #[test]
    fn test_component_id_display() {
        let id = ComponentId::new("npm", ".");
        assert_eq!(id.to_string(), "component::npm::.");
    }

    #[test]
    fn test_component_id_roundtrip() {
        let id = ComponentId::new("cargo", "crates/chizu-core");
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: ComponentId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_symbol_id() {
        assert_eq!(
            symbol_id("src/main.rs", "main"),
            "symbol::src/main.rs::main"
        );
    }

    #[test]
    fn test_source_unit_id() {
        assert_eq!(source_unit_id("src/lib.rs"), "source_unit::src/lib.rs");
    }

    #[test]
    fn test_component_id_func() {
        assert_eq!(
            component_id("cargo", "crates/core"),
            "component::cargo::crates/core"
        );
    }

    #[test]
    fn test_hash_to_i64() {
        let hash = blake3::hash(b"test");
        let key = hash_to_i64(&hash);
        // Just verify it produces a value
        assert_ne!(key, 0);
    }

    #[test]
    fn test_entity_id_to_usearch_key() {
        let key1 = entity_id_to_usearch_key("symbol::src/main.rs::main");
        let key2 = entity_id_to_usearch_key("symbol::src/main.rs::main");
        let key3 = entity_id_to_usearch_key("symbol::src/lib.rs::other");

        // Same input should produce same key
        assert_eq!(key1, key2);
        // Different input should likely produce different key
        assert_ne!(key1, key3);
    }
}
