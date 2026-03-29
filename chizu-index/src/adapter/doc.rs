use chizu_core::{Edge, EdgeKind, Entity, EntityKind, doc_id};

use crate::error::Result;
use crate::walk::WalkedFile;

/// Index a documentation file.
pub fn index_doc_file(file: &WalkedFile) -> Result<(Vec<Entity>, Vec<Edge>)> {
    let mut entities = Vec::new();
    let mut edges = Vec::new();
    let path_str = file.path.to_string_lossy();

    let id = doc_id(&path_str);
    let name = file
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&path_str);

    entities.push(
        Entity::new(&id, EntityKind::Doc, name)
            .with_path(path_str.as_ref())
            .with_exported(true),
    );

    let src = file
        .component_id
        .as_ref()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "repo::.".to_string());
    edges.push(Edge::new(&src, EdgeKind::DocumentedBy, &id));

    Ok((entities, edges))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::ComponentId;
    use std::path::PathBuf;

    #[test]
    fn doc_adapter_emits_documented_by_from_component() {
        let file = WalkedFile {
            path: PathBuf::from("crates/core/README.md"),
            hash: "abc".to_string(),
            component_id: Some(ComponentId::new("cargo", "crates/core")),
        };
        let (entities, edges) = index_doc_file(&file).unwrap();
        assert_eq!(entities[0].id, "doc::crates/core/README.md");
        assert_eq!(entities[0].kind, EntityKind::Doc);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].src_id, "component::cargo::crates/core");
        assert_eq!(edges[0].rel, EdgeKind::DocumentedBy);
    }

    #[test]
    fn doc_adapter_emits_documented_by_from_repo() {
        let file = WalkedFile {
            path: PathBuf::from("README.md"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (_, edges) = index_doc_file(&file).unwrap();
        assert_eq!(edges[0].src_id, "repo::.");
    }
}
