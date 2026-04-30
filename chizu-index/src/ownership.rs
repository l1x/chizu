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

/// Build a component registry from discovered package.json paths.
///
/// V1 scope: literal paths in the `workspaces` array. Glob patterns,
/// pnpm-workspace.yaml, and Yarn nested workspaces are deferred.
pub fn discover_npm_components(
    files: &[WalkedFile],
    repo_root: &Path,
) -> crate::error::Result<ComponentRegistry> {
    let mut registry = ComponentRegistry::new();

    for file in files {
        if file.path.file_name() != Some(std::ffi::OsStr::new("package.json")) {
            continue;
        }

        let abs_path = repo_root.join(&file.path);
        let content = std::fs::read_to_string(&abs_path)?;
        let manifest: serde_json::Value = serde_json::from_str(&content)?;

        let package_name = manifest.get("name").and_then(|n| n.as_str());

        let name = match package_name {
            Some(n) => n.to_string(),
            None => continue,
        };

        let parent = file.path.parent().unwrap_or(Path::new(""));
        registry.register(parent.to_path_buf(), name.clone(), "npm");

        // Register workspace members (literal paths only)
        if let Some(workspaces) = manifest.get("workspaces").and_then(|w| w.as_array()) {
            for ws in workspaces {
                if let Some(ws_path) = ws.as_str() {
                    let resolved = parent.join(ws_path);
                    // Try to read the member's package.json to get its name
                    let member_pkg = repo_root.join(&resolved).join("package.json");
                    if let Ok(member_content) = std::fs::read_to_string(member_pkg)
                        && let Ok(member_manifest) =
                            serde_json::from_str::<serde_json::Value>(&member_content)
                        && let Some(member_name) =
                            member_manifest.get("name").and_then(|n| n.as_str())
                    {
                        registry.register(resolved, member_name.to_string(), "npm");
                    }
                }
            }
        }
    }

    Ok(registry)
}

/// Build a combined component registry from all supported ecosystems.
pub fn discover_all_components(
    files: &[WalkedFile],
    repo_root: &Path,
) -> crate::error::Result<ComponentRegistry> {
    let mut cargo_registry = discover_cargo_components(files, repo_root)?;
    let npm_registry = discover_npm_components(files, repo_root)?;
    cargo_registry.merge_from(npm_registry);
    Ok(cargo_registry)
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

    #[test]
    fn discover_npm_components_finds_packages() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("packages/foo")).unwrap();
        fs::create_dir_all(root.join("packages/bar")).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{
  "name": "root-pkg",
  "workspaces": ["packages/foo", "packages/bar"]
}"#,
        )
        .unwrap();
        fs::write(root.join("packages/foo/package.json"), r#"{"name": "foo"}"#).unwrap();
        fs::write(root.join("packages/bar/package.json"), r#"{"name": "bar"}"#).unwrap();

        let config = Config::default();
        let walker = FileWalker::new(root, &config).unwrap();
        let files = walker.walk().unwrap();

        let registry = discover_npm_components(&files, root).unwrap();

        assert_eq!(
            registry.resolve_name("root-pkg"),
            Some(&ComponentId::new("npm", "."))
        );
        assert_eq!(
            registry.resolve_name("foo"),
            Some(&ComponentId::new("npm", "packages/foo"))
        );
        assert_eq!(
            registry.resolve_name("bar"),
            Some(&ComponentId::new("npm", "packages/bar"))
        );
    }

    #[test]
    fn discover_all_components_merges_ecosystems() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::create_dir_all(root.join("crates/core")).unwrap();
        fs::create_dir_all(root.join("packages/web")).unwrap();
        fs::write(
            root.join("crates/core/Cargo.toml"),
            r#"[package]
name = "core"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::write(root.join("packages/web/package.json"), r#"{"name": "web"}"#).unwrap();

        let config = Config::default();
        let walker = FileWalker::new(root, &config).unwrap();
        let files = walker.walk().unwrap();

        let registry = discover_all_components(&files, root).unwrap();

        assert_eq!(
            registry.resolve_name("core"),
            Some(&ComponentId::new("cargo", "crates/core"))
        );
        assert_eq!(
            registry.resolve_name("web"),
            Some(&ComponentId::new("npm", "packages/web"))
        );
    }
}
