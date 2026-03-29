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
pub struct ComponentId(pub(crate) String);

impl ComponentId {
    pub fn new(ecosystem: &str, path: &str) -> Self {
        Self(format!("component::{}::{}", ecosystem, path))
    }

    /// Parse a component ID from its string representation.
    /// Returns `None` if the string doesn't start with `component::`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::try_from(s.to_string()).ok()
    }

    pub fn ecosystem(&self) -> Option<&str> {
        self.0.strip_prefix("component::")?.split("::").next()
    }

    /// For `component::cargo::crates/core` returns `Some("crates/core")`.
    /// Handles paths containing `::` (e.g. `component::terraform::modules::vpc`
    /// returns `Some("modules::vpc")`).
    pub fn path(&self) -> Option<&str> {
        let rest = self.0.strip_prefix("component::")?;
        let (_ecosystem, path) = rest.split_once("::")?;
        Some(path)
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ComponentId {
    type Error = String;

    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        if s.starts_with("component::") {
            Ok(Self(s))
        } else {
            Err(format!(
                "invalid component ID: must start with 'component::' but got '{}'",
                s
            ))
        }
    }
}

impl From<ComponentId> for String {
    fn from(id: ComponentId) -> Self {
        id.0
    }
}

pub fn symbol_id(file_path: &str, name: &str) -> String {
    format!("symbol::{}::{}", file_path, name)
}

pub fn source_unit_id(path: &str) -> String {
    format!("source_unit::{}", path)
}

pub fn component_id(ecosystem: &str, path: &str) -> String {
    ComponentId::new(ecosystem, path).into()
}

pub fn test_id(file_path: &str, name: &str) -> String {
    format!("test::{}::{}", file_path, name)
}

pub fn doc_id(path: &str) -> String {
    format!("doc::{}", path)
}

/// Convert a blake3 hash to an i64 for usearch key mapping.
///
/// Birthday collision probability: ~n²/2^65 — at 1M entities ≈ 5.4×10⁻⁸.
pub fn hash_to_i64(hash: &blake3::Hash) -> i64 {
    let bytes = hash.as_bytes();
    i64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

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
        assert_eq!(id.as_str(), "component::cargo::crates/core");
    }

    #[test]
    fn test_component_id_parse() {
        let id = ComponentId::parse("component::npm::packages/web").unwrap();
        assert_eq!(id.as_str(), "component::npm::packages/web");
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
    fn test_component_id_path_with_double_colon() {
        let id = ComponentId::parse("component::terraform::modules::vpc").unwrap();
        assert_eq!(id.path(), Some("modules::vpc"));
    }

    #[test]
    fn test_component_id_path_root() {
        let id = ComponentId::new("npm", ".");
        assert_eq!(id.path(), Some("."));
    }

    #[test]
    fn test_component_id_try_from_valid() {
        let id = ComponentId::try_from("component::cargo::core".to_string());
        assert!(id.is_ok());
        assert_eq!(id.unwrap().as_str(), "component::cargo::core");
    }

    #[test]
    fn test_component_id_try_from_invalid() {
        let id = ComponentId::try_from("garbage".to_string());
        assert!(id.is_err());
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
        assert_ne!(key, 0);
    }

    #[test]
    fn test_entity_id_to_usearch_key() {
        let key1 = entity_id_to_usearch_key("symbol::src/main.rs::main");
        let key2 = entity_id_to_usearch_key("symbol::src/main.rs::main");
        let key3 = entity_id_to_usearch_key("symbol::src/lib.rs::other");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }
}
