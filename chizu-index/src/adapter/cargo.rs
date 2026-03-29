use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chizu_core::{ComponentId, Edge, EdgeKind, Entity, EntityKind};

use crate::error::{IndexError, Result};
use crate::registry::ComponentRegistry;

/// Facts extracted from a Cargo workspace.
#[derive(Debug, Default)]
pub struct CargoFacts {
    pub entities: Vec<Entity>,
    pub edges: Vec<Edge>,
}

/// Parse Cargo.toml files and emit entities/edges for the workspace.
pub fn index_cargo_workspace(repo_root: &Path, registry: &ComponentRegistry) -> Result<CargoFacts> {
    let mut facts = CargoFacts::default();

    let repo_id = "repo::.".to_string();
    facts.entities.push(
        Entity::new(&repo_id, EntityKind::Repo, "repo")
            .with_path(".")
            .with_exported(true),
    );

    let mut manifests: Vec<(PathBuf, CargoManifest)> = Vec::new();
    for (path, _) in registry.all_components() {
        let manifest_path = repo_root.join(path).join("Cargo.toml");
        let content = std::fs::read_to_string(&manifest_path)?;
        let manifest: CargoManifest =
            toml::from_str(&content).map_err(|e| IndexError::InvalidManifest {
                path: manifest_path.clone(),
                message: e.to_string(),
            })?;
        manifests.push((path.clone(), manifest));
    }

    for (rel_path, manifest) in &manifests {
        let package = match &manifest.package {
            Some(p) => p,
            None => continue,
        };

        let comp_path = normalize_component_path(rel_path);
        let comp_id_str = ComponentId::new("cargo", &comp_path).to_string();

        facts.entities.push(
            Entity::new(&comp_id_str, EntityKind::Component, &package.name)
                .with_path(rel_path.to_string_lossy().to_string())
                .with_exported(true),
        );

        facts
            .edges
            .push(Edge::new(&repo_id, EdgeKind::Contains, &comp_id_str));

        let deps = merge_deps(&manifest.dependencies, &manifest.dev_dependencies);
        for (dep_name, dep_value) in deps {
            if let Some(target) =
                resolve_local_dependency(repo_root, registry, rel_path, dep_name, dep_value)
            {
                facts.edges.push(Edge::new(
                    &comp_id_str,
                    EdgeKind::DependsOn,
                    target.to_string(),
                ));
            }
        }

        if let Some(features) = &manifest.features {
            for (feature_name, enables) in features {
                let feat_id = feature_id(&comp_path, feature_name);
                facts.entities.push(
                    Entity::new(&feat_id, EntityKind::Feature, feature_name)
                        .with_path(rel_path.to_string_lossy().to_string())
                        .with_exported(true),
                );
                facts.edges.push(Edge::new(
                    &comp_id_str,
                    EdgeKind::DeclaresFeature,
                    &feat_id,
                ));

                for enabled in enables {
                    // Skip dependency features (dep:crate) and cross-crate
                    // feature enables (dep-name/feature-name). The '/' syntax
                    // is used by Cargo for external feature references.
                    if enabled.starts_with("dep:") || enabled.contains('/') {
                        continue;
                    }
                    let enabled_id = feature_id(&comp_path, enabled);
                    facts
                        .edges
                        .push(Edge::new(&feat_id, EdgeKind::FeatureEnables, &enabled_id));
                }
            }
        }
    }

    Ok(facts)
}

/// Normalize a repo-relative path for use in component IDs.
/// Empty or "." paths become "."; everything else uses lossy string conversion.
pub(crate) fn normalize_component_path(path: &Path) -> String {
    if path == Path::new("") || path == Path::new(".") {
        ".".to_string()
    } else {
        path.to_string_lossy().to_string()
    }
}

fn feature_id(component_path: &str, feature_name: &str) -> String {
    format!("feature::{component_path}::{feature_name}")
}

fn merge_deps<'a>(
    deps: &'a Option<HashMap<String, CargoDependency>>,
    dev_deps: &'a Option<HashMap<String, CargoDependency>>,
) -> impl Iterator<Item = (&'a String, &'a CargoDependency)> {
    deps.iter()
        .flat_map(|d| d.iter())
        .chain(dev_deps.iter().flat_map(|d| d.iter()))
}

fn resolve_local_dependency(
    repo_root: &Path,
    registry: &ComponentRegistry,
    source_path: &Path,
    dep_name: &str,
    dep: &CargoDependency,
) -> Option<ComponentId> {
    if let Some(path_str) = dep.path() {
        let resolved = normalize_path(&repo_root.join(source_path).join(path_str));
        // strip_prefix fails if the normalized path escapes repo_root
        // (e.g., via ../../). In that case we treat it as unresolvable.
        let rel = resolved.strip_prefix(repo_root).ok()?;
        return registry.component_for_path(rel).cloned();
    }
    registry.resolve_name(dep_name).cloned()
}

/// Resolve `.` and `..` components without hitting the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            _ => result.push(component),
        }
    }
    result
}

// ── Cargo TOML deserialization types ────────────────────────────────────
// Fields that are never read by application logic are still required for
// correct serde deserialization (especially `#[serde(untagged)]` on
// CargoDependency, which needs both variants to parse correctly).

#[derive(Debug, serde::Deserialize)]
struct CargoManifest {
    package: Option<CargoPackage>,
    #[allow(dead_code)]
    workspace: Option<CargoWorkspace>,
    dependencies: Option<HashMap<String, CargoDependency>>,
    #[serde(rename = "dev-dependencies")]
    dev_dependencies: Option<HashMap<String, CargoDependency>>,
    features: Option<HashMap<String, Vec<String>>>,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct CargoPackage {
    name: String,
    version: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct CargoWorkspace {
    members: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum CargoDependency {
    Simple(String),
    Detailed {
        path: Option<String>,
        version: Option<String>,
        #[serde(rename = "optional")]
        optional: Option<bool>,
    },
}

impl CargoDependency {
    fn path(&self) -> Option<&str> {
        match self {
            CargoDependency::Simple(_) => None,
            CargoDependency::Detailed { path, .. } => path.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn cargo_adapter_single_crate() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "my-app"
version = "0.1.0"

[dependencies]
"#,
        )
        .unwrap();

        let mut registry = ComponentRegistry::new();
        registry.register(PathBuf::from(""), "my-app".to_string(), "cargo");

        let facts = index_cargo_workspace(root, &registry).unwrap();

        assert!(facts.entities.iter().any(|e| e.kind == EntityKind::Repo));
        let comp = facts
            .entities
            .iter()
            .find(|e| e.id == "component::cargo::.");
        assert!(comp.is_some());
        assert_eq!(comp.unwrap().name, "my-app");
        assert!(facts
            .edges
            .iter()
            .any(|e| e.src_id == "repo::." && e.dst_id == "component::cargo::."));
    }

    #[test]
    fn cargo_adapter_workspace_with_features() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("Cargo.toml"),
            r#"[workspace]
members = ["crates/foo", "crates/bar"]
"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("crates/foo")).unwrap();
        fs::write(
            root.join("crates/foo/Cargo.toml"),
            r#"[package]
name = "foo"
version = "0.1.0"

[features]
default = ["std"]
std = []
"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("crates/bar")).unwrap();
        fs::write(
            root.join("crates/bar/Cargo.toml"),
            r#"[package]
name = "bar"
version = "0.1.0"

[dependencies]
foo = { path = "../foo" }
"#,
        )
        .unwrap();

        let mut registry = ComponentRegistry::new();
        registry.register(PathBuf::from("crates/foo"), "foo".to_string(), "cargo");
        registry.register(PathBuf::from("crates/bar"), "bar".to_string(), "cargo");

        let facts = index_cargo_workspace(root, &registry).unwrap();

        let comps: Vec<_> = facts
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::Component)
            .collect();
        assert_eq!(comps.len(), 2);

        assert!(facts.edges.iter().any(|e| e.src_id
            == "component::cargo::crates/bar"
            && e.dst_id == "component::cargo::crates/foo"
            && e.rel == EdgeKind::DependsOn));

        assert!(facts
            .entities
            .iter()
            .any(|e| e.id == "feature::crates/foo::std"));
        assert!(facts
            .entities
            .iter()
            .any(|e| e.id == "feature::crates/foo::default"));

        assert!(facts.edges.iter().any(|e| e.src_id
            == "component::cargo::crates/foo"
            && e.dst_id == "feature::crates/foo::default"
            && e.rel == EdgeKind::DeclaresFeature));

        assert!(facts.edges.iter().any(|e| e.src_id
            == "feature::crates/foo::default"
            && e.dst_id == "feature::crates/foo::std"
            && e.rel == EdgeKind::FeatureEnables));
    }
}
