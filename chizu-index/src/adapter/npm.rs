use std::path::{Path, PathBuf};

use chizu_core::{ComponentId, Edge, EdgeKind, Entity, EntityKind};

use super::AdapterFacts;
use crate::adapter::cargo::normalize_component_path;
use crate::error::Result;
use crate::registry::ComponentRegistry;

/// Parse package.json files and emit entities/edges for the workspace.
pub fn index_npm_workspace(repo_root: &Path, registry: &ComponentRegistry) -> Result<AdapterFacts> {
    let mut facts = AdapterFacts::default();
    let mut has_workspace = false;
    let mut manifests: Vec<(PathBuf, serde_json::Value)> = Vec::new();

    for (path, comp_id) in registry.all_components() {
        if !comp_id.as_str().starts_with("component::npm::") {
            continue;
        }
        if !path.join("package.json").exists() && path.as_os_str().is_empty() {
            // root component
            let manifest_path = repo_root.join("package.json");
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                let manifest: serde_json::Value = serde_json::from_str(&content)?;
                manifests.push((path.clone(), manifest));
            }
        } else {
            let manifest_path = repo_root.join(path).join("package.json");
            let content = std::fs::read_to_string(&manifest_path)?;
            let manifest: serde_json::Value = serde_json::from_str(&content)?;
            manifests.push((path.clone(), manifest));
        }
    }

    for (rel_path, manifest) in &manifests {
        let package_name = manifest
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        let comp_path = normalize_component_path(rel_path);
        let comp_id_str = ComponentId::new("npm", &comp_path).to_string();

        facts.entities.push(
            Entity::new(&comp_id_str, EntityKind::Component, package_name)
                .with_path(rel_path.to_string_lossy().to_string())
                .with_exported(true),
        );

        if rel_path.as_os_str().is_empty() {
            has_workspace = true;
        }

        let deps = merge_deps(&manifest);
        for dep_name in deps {
            if let Some(target) = registry.resolve_name(dep_name) {
                facts.edges.push(Edge::new(
                    &comp_id_str,
                    EdgeKind::DependsOn,
                    target.to_string(),
                ));
            }
        }
    }

    // Emit contains edges from repo to all npm components
    if has_workspace || !manifests.is_empty() {
        for (rel_path, _) in &manifests {
            let comp_path = normalize_component_path(rel_path);
            let comp_id_str = ComponentId::new("npm", &comp_path).to_string();
            facts
                .edges
                .push(Edge::new("repo::.", EdgeKind::Contains, &comp_id_str));
        }
    }

    Ok(facts)
}

fn merge_deps(manifest: &serde_json::Value) -> Vec<&str> {
    let mut deps = Vec::new();
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(obj) = manifest.get(key).and_then(|d| d.as_object()) {
            for name in obj.keys() {
                deps.push(name.as_str());
            }
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn npm_adapter_workspace_with_deps() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("package.json"),
            r#"{
  "name": "root-pkg",
  "workspaces": ["packages/foo", "packages/bar"]
}"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("packages/foo")).unwrap();
        fs::write(
            root.join("packages/foo/package.json"),
            r#"{"name": "foo", "dependencies": {"bar": "1.0.0"}}"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("packages/bar")).unwrap();
        fs::write(
            root.join("packages/bar/package.json"),
            r#"{"name": "bar"}"#,
        )
        .unwrap();

        let mut registry = ComponentRegistry::new();
        registry.register(PathBuf::from(""), "root-pkg".to_string(), "npm");
        registry.register(PathBuf::from("packages/foo"), "foo".to_string(), "npm");
        registry.register(PathBuf::from("packages/bar"), "bar".to_string(), "npm");

        let facts = index_npm_workspace(root, &registry).unwrap();

        assert!(facts.entities.iter().any(|e| e.id == "component::npm::."));
        assert!(facts.entities.iter().any(|e| e.id == "component::npm::packages/foo"));
        assert!(facts.entities.iter().any(|e| e.id == "component::npm::packages/bar"));

        assert!(facts.edges.iter().any(|e| {
            e.src_id == "component::npm::packages/foo"
                && e.rel == EdgeKind::DependsOn
                && e.dst_id == "component::npm::packages/bar"
        }));

        assert!(facts.edges.iter().any(|e| {
            e.src_id == "repo::." && e.rel == EdgeKind::Contains && e.dst_id == "component::npm::packages/foo"
        }));
    }
}
