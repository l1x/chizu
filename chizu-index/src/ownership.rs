use crate::registry::ComponentRegistry;
use crate::walk::WalkedFile;
use std::path::Path;

/// Assign component ownership to walked files using longest-prefix matching.
pub fn assign_ownership(files: &mut [WalkedFile], registry: &ComponentRegistry) {
    for file in files {
        file.component_id = registry.component_for_path(&file.path).cloned();
    }
}

/// Build a component registry from discovered Cargo.toml paths.
pub fn discover_cargo_components(
    files: &[WalkedFile],
    repo_root: &Path,
) -> crate::error::Result<ComponentRegistry> {
    let mut registry = ComponentRegistry::new();

    for file in files {
        if file.path.file_name() != Some(std::ffi::OsStr::new("Cargo.toml")) {
            continue;
        }

        let abs_path = repo_root.join(&file.path);
        let content = std::fs::read_to_string(&abs_path)?;
        let manifest: toml::Value = content.parse()?;

        let package_name = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str());

        let name = match package_name {
            Some(n) => n.to_string(),
            None => continue, // workspace root Cargo.toml may not have [package]
        };

        let parent = file.path.parent().unwrap_or(Path::new(""));
        registry.register(parent.to_path_buf(), name, "cargo");
    }

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::{FileWalker, WalkedFile};
    use chizu_core::{ComponentId, Config};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn assign_ownership_from_registry() {
        let mut registry = ComponentRegistry::new();
        registry.register(
            PathBuf::from("crates/core"),
            "chizu-core".to_string(),
            "cargo",
        );

        let mut files = vec![
            WalkedFile {
                path: PathBuf::from("crates/core/src/lib.rs"),
                hash: "abc".to_string(),
                component_id: None,
            },
            WalkedFile {
                path: PathBuf::from("README.md"),
                hash: "def".to_string(),
                component_id: None,
            },
        ];

        assign_ownership(&mut files, &registry);

        assert_eq!(
            files[0].component_id,
            Some(ComponentId::new("cargo", "crates/core"))
        );
        assert_eq!(files[1].component_id, None);
    }

    #[test]
    fn discover_cargo_components_finds_crates() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("crates/foo")).unwrap();
        fs::create_dir_all(root.join("crates/bar")).unwrap();
        fs::write(
            root.join("crates/foo/Cargo.toml"),
            r#"[package]
name = "foo"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::write(
            root.join("crates/bar/Cargo.toml"),
            r#"[package]
name = "bar"
version = "0.1.0"
"#,
        )
        .unwrap();

        let config = Config::default();
        let walker = FileWalker::new(root, &config).unwrap();
        let files = walker.walk().unwrap();

        let registry = discover_cargo_components(&files, root).unwrap();

        assert_eq!(
            registry.resolve_name("foo"),
            Some(&ComponentId::new("cargo", "crates/foo"))
        );
        assert_eq!(
            registry.resolve_name("bar"),
            Some(&ComponentId::new("cargo", "crates/bar"))
        );
    }
}
