use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chizu_core::ComponentId;

use crate::adapter::cargo::normalize_component_path;

/// Registry mapping component root paths and names to canonical component IDs.
#[derive(Debug, Clone, Default)]
pub struct ComponentRegistry {
    by_path: HashMap<PathBuf, ComponentId>,
    by_name: HashMap<String, ComponentId>,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a component root.
    /// `path` must be repo-relative. `ecosystem` is used for ID namespacing.
    pub fn register(&mut self, path: PathBuf, name: String, ecosystem: &str) {
        let id = ComponentId::new(ecosystem, &normalize_component_path(&path));
        self.by_name.insert(name, id.clone());
        self.by_path.insert(path, id);
    }

    /// Look up the most specific enclosing component for a file path.
    pub fn component_for_path(&self, file_path: &Path) -> Option<&ComponentId> {
        let mut best: Option<(&PathBuf, &ComponentId)> = None;
        for (root, id) in &self.by_path {
            if file_path.starts_with(root) {
                match best {
                    Some((best_root, _))
                        if root.components().count() > best_root.components().count() =>
                    {
                        best = Some((root, id));
                    }
                    None => {
                        best = Some((root, id));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, id)| id)
    }

    /// Resolve a manifest display name to a component ID.
    pub fn resolve_name(&self, name: &str) -> Option<&ComponentId> {
        self.by_name.get(name)
    }

    /// Iterate over all registered components.
    pub fn all_components(&self) -> impl Iterator<Item = (&PathBuf, &ComponentId)> {
        self.by_path.iter()
    }

    /// Merge another registry into this one.
    pub fn merge_from(&mut self, other: ComponentRegistry) {
        self.by_path.extend(other.by_path);
        self.by_name.extend(other.by_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_workspace_with_two_crates() {
        let mut registry = ComponentRegistry::new();
        registry.register(PathBuf::from("crates/foo"), "foo".to_string(), "cargo");
        registry.register(PathBuf::from("crates/bar"), "bar".to_string(), "cargo");

        let foo_id = ComponentId::new("cargo", "crates/foo");
        let bar_id = ComponentId::new("cargo", "crates/bar");

        assert_eq!(registry.resolve_name("foo"), Some(&foo_id));
        assert_eq!(registry.resolve_name("bar"), Some(&bar_id));

        assert_eq!(
            registry.component_for_path(Path::new("crates/foo/src/lib.rs")),
            Some(&foo_id)
        );
        assert_eq!(
            registry.component_for_path(Path::new("crates/bar/src/main.rs")),
            Some(&bar_id)
        );
        assert_eq!(
            registry.component_for_path(Path::new("docs/readme.md")),
            None
        );
    }

    #[test]
    fn root_component_uses_dot() {
        let mut registry = ComponentRegistry::new();
        registry.register(PathBuf::from(""), "root".to_string(), "npm");

        let root_id = ComponentId::new("npm", ".");
        assert_eq!(registry.resolve_name("root"), Some(&root_id));
        assert_eq!(
            registry.component_for_path(Path::new("src/index.js")),
            Some(&root_id)
        );
    }
}
