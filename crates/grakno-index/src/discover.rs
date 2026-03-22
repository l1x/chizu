use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cargo_metadata::MetadataCommand;

use crate::error::IndexError;

#[derive(Debug, Clone)]
pub struct DiscoveredWorkspace {
    pub name: String,
    pub root: PathBuf,
    pub crates: Vec<DiscoveredCrate>,
    pub deps: Vec<CrateDep>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredCrate {
    pub name: String,
    pub manifest_dir: PathBuf,
    pub features: Vec<DiscoveredFeature>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredFeature {
    pub name: String,
    pub enables: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CrateDep {
    pub from: String,
    pub to: String,
}

pub fn discover(path: &Path) -> Result<DiscoveredWorkspace, IndexError> {
    let manifest = path.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec()?;

    let workspace_root = metadata.workspace_root.as_std_path().to_path_buf();
    let workspace_name = workspace_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let member_ids: HashSet<_> = metadata.workspace_members.iter().collect();
    let member_names: HashSet<String> = metadata
        .packages
        .iter()
        .filter(|p| member_ids.contains(&p.id))
        .map(|p| p.name.clone())
        .collect();

    let mut crates = Vec::new();
    let mut deps = Vec::new();

    for pkg in &metadata.packages {
        if !member_ids.contains(&pkg.id) {
            continue;
        }

        let manifest_dir = pkg
            .manifest_path
            .parent()
            .map(|p| p.as_std_path().to_path_buf())
            .unwrap_or_default();

        let features = pkg
            .features
            .iter()
            .map(|(name, enables)| DiscoveredFeature {
                name: name.clone(),
                enables: enables.clone(),
            })
            .collect();

        crates.push(DiscoveredCrate {
            name: pkg.name.clone(),
            manifest_dir,
            features,
        });

        for dep in &pkg.dependencies {
            if member_names.contains(&dep.name) {
                deps.push(CrateDep {
                    from: pkg.name.clone(),
                    to: dep.name.clone(),
                });
            }
        }
    }

    // Sort crates by name for deterministic ordering
    crates.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(DiscoveredWorkspace {
        name: workspace_name,
        root: workspace_root,
        crates,
        deps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_grakno_workspace() {
        // Discover the grakno workspace itself
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let ws = discover(root).unwrap();

        assert_eq!(ws.name, "grakno");
        assert!(ws.crates.len() >= 2);

        let names: Vec<_> = ws.crates.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"grakno-core"));
        assert!(names.contains(&"grakno-index"));

        // grakno-index depends on grakno-core
        let has_dep = ws
            .deps
            .iter()
            .any(|d| d.from == "grakno-index" && d.to == "grakno-core");
        assert!(has_dep, "expected grakno-index -> grakno-core dep");
    }
}
