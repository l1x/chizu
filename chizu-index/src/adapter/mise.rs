use chizu_core::{Edge, EdgeKind, Entity, EntityKind, entity_id};

use crate::error::Result;
use crate::walk::WalkedFile;

/// Index a mise.toml file and extract tasks.
pub fn index_mise_file(
    file: &WalkedFile,
    repo_root: &std::path::Path,
) -> Result<(Vec<Entity>, Vec<Edge>)> {
    let mut entities = Vec::new();
    let mut edges = Vec::new();
    let path_str = file.path.to_string_lossy();

    let abs_path = repo_root.join(&file.path);
    let content = std::fs::read_to_string(&abs_path)?;
    let manifest: toml::Value = content.parse()?;

    if let Some(tasks) = manifest.get("tasks").and_then(|t| t.as_table()) {
        for (task_name, _) in tasks {
            let id = entity_id("task", &format!("{}::{}", path_str, task_name));
            let mut entity = Entity::new(&id, EntityKind::Task, task_name)
                .with_path(path_str.as_ref())
                .with_exported(true);
            if let Some(component_id) = file.component_id.as_ref() {
                entity = entity.with_component(component_id.clone());
            }
            entities.push(entity);
            edges.push(Edge::new("repo::.", EdgeKind::OwnsTask, &id));
        }
    }

    Ok((entities, edges))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn mise_adapter_extracts_tasks() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("mise.toml"),
            r#"[tasks]
build = "cargo build"
test = "cargo test"
deploy = "./deploy.sh"
"#,
        )
        .unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("mise.toml"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (entities, edges) = index_mise_file(&file, root).unwrap();
        assert_eq!(entities.len(), 3);
        assert!(entities.iter().any(|e| e.name == "build"));
        assert!(entities.iter().any(|e| e.name == "test"));
        assert!(entities.iter().any(|e| e.name == "deploy"));
        assert_eq!(edges.len(), 3);
        assert!(edges.iter().all(|e| e.rel == EdgeKind::OwnsTask));
    }
}
