use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::IndexError;

#[derive(Debug, Clone, Deserialize)]
pub struct PackageJson {
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub dependencies: HashMap<String, serde_json::Value>,
    #[serde(rename = "devDependencies", default)]
    pub dev_dependencies: HashMap<String, serde_json::Value>,
    #[serde(rename = "peerDependencies", default)]
    pub peer_dependencies: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub workspaces: Option<WorkspacesConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WorkspacesConfig {
    Array(Vec<String>),
    Object { packages: Vec<String> },
}

/// Parse a package.json file from its source text.
pub fn parse_package_json(source: &str) -> Result<PackageJson, IndexError> {
    serde_json::from_str(source).map_err(|e| IndexError::Parse(format!("package.json: {e}")))
}

/// Resolve workspace glob patterns relative to a root directory.
/// Returns the list of directories that match the workspace patterns.
pub fn resolve_workspaces(root: &Path, config: &WorkspacesConfig) -> Vec<PathBuf> {
    let patterns = match config {
        WorkspacesConfig::Array(arr) => arr.clone(),
        WorkspacesConfig::Object { packages } => packages.clone(),
    };

    let mut results = Vec::new();
    for pattern in &patterns {
        let full_pattern = root.join(pattern).display().to_string();
        if let Ok(paths) = glob::glob(&full_pattern) {
            for entry in paths.flatten() {
                // Only include directories that contain a package.json
                if entry.is_dir() && entry.join("package.json").exists() {
                    results.push(entry);
                }
            }
        }
    }
    results.sort();
    results.dedup();
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_package_json() {
        let src = r#"{
            "name": "my-app",
            "version": "1.0.0",
            "dependencies": {
                "express": "^4.18.0"
            }
        }"#;
        let pkg = parse_package_json(src).unwrap();
        assert_eq!(pkg.name.as_deref(), Some("my-app"));
        assert_eq!(pkg.version.as_deref(), Some("1.0.0"));
        assert_eq!(pkg.dependencies.len(), 1);
        assert!(pkg.dependencies.contains_key("express"));
    }

    #[test]
    fn parse_all_dep_types() {
        let src = r#"{
            "name": "full-deps",
            "dependencies": {
                "react": "^18.0.0",
                "react-dom": "^18.0.0"
            },
            "devDependencies": {
                "typescript": "^5.0.0",
                "jest": "^29.0.0"
            },
            "peerDependencies": {
                "react": ">=16.0.0"
            }
        }"#;
        let pkg = parse_package_json(src).unwrap();
        assert_eq!(pkg.dependencies.len(), 2);
        assert_eq!(pkg.dev_dependencies.len(), 2);
        assert_eq!(pkg.peer_dependencies.len(), 1);
    }

    #[test]
    fn parse_workspaces_array() {
        let src = r#"{
            "name": "monorepo",
            "workspaces": ["packages/*", "apps/*"]
        }"#;
        let pkg = parse_package_json(src).unwrap();
        match pkg.workspaces.unwrap() {
            WorkspacesConfig::Array(arr) => {
                assert_eq!(arr, vec!["packages/*", "apps/*"]);
            }
            other => panic!("expected Array, got {:?}", other),
        }
    }

    #[test]
    fn parse_workspaces_object() {
        let src = r#"{
            "name": "monorepo",
            "workspaces": {
                "packages": ["packages/*"]
            }
        }"#;
        let pkg = parse_package_json(src).unwrap();
        match pkg.workspaces.unwrap() {
            WorkspacesConfig::Object { packages } => {
                assert_eq!(packages, vec!["packages/*"]);
            }
            other => panic!("expected Object, got {:?}", other),
        }
    }

    #[test]
    fn parse_minimal_package_json() {
        let src = r#"{}"#;
        let pkg = parse_package_json(src).unwrap();
        assert!(pkg.name.is_none());
        assert!(pkg.version.is_none());
        assert!(pkg.dependencies.is_empty());
        assert!(pkg.dev_dependencies.is_empty());
        assert!(pkg.peer_dependencies.is_empty());
        assert!(pkg.workspaces.is_none());
    }

    #[test]
    fn parse_scoped_package_names() {
        let src = r#"{
            "name": "@scope/my-pkg",
            "dependencies": {
                "@angular/core": "^17.0.0",
                "@types/node": "^20.0.0"
            }
        }"#;
        let pkg = parse_package_json(src).unwrap();
        assert_eq!(pkg.name.as_deref(), Some("@scope/my-pkg"));
        assert!(pkg.dependencies.contains_key("@angular/core"));
        assert!(pkg.dependencies.contains_key("@types/node"));
    }

    #[test]
    fn resolve_workspaces_with_temp_dir() {
        let tmp = std::env::temp_dir().join("grakno_test_ws");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("packages/foo")).unwrap();
        std::fs::write(tmp.join("packages/foo/package.json"), "{}").unwrap();
        std::fs::create_dir_all(tmp.join("packages/bar")).unwrap();
        std::fs::write(tmp.join("packages/bar/package.json"), "{}").unwrap();
        // A directory without package.json should be excluded
        std::fs::create_dir_all(tmp.join("packages/no-pkg")).unwrap();

        let config = WorkspacesConfig::Array(vec!["packages/*".to_string()]);
        let resolved = resolve_workspaces(&tmp, &config);

        assert_eq!(resolved.len(), 2);
        assert!(resolved.iter().any(|p| p.ends_with("foo")));
        assert!(resolved.iter().any(|p| p.ends_with("bar")));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
