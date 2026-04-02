pub mod cargo;
pub mod doc;
pub mod frontmatter;
pub mod mise;
pub mod npm;
pub mod rust;
pub mod scanner;
pub mod site;

use std::path::Path;

use chizu_core::{Edge, Entity};

use crate::error::Result;
use crate::walk::WalkedFile;

/// Entities and edges returned by any workspace/file adapter.
#[derive(Debug, Default)]
pub struct AdapterFacts {
    pub entities: Vec<Entity>,
    pub edges: Vec<Edge>,
}

/// Check whether a path has a component matching a known content directory name.
pub(crate) fn is_content_dir(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str();
        s == "content" || s == "pages" || s == "blog"
    })
}

/// Dispatch a file to the appropriate adapter(s) and return emitted entities/edges.
pub fn index_file(
    repo_root: &Path,
    file: &WalkedFile,
) -> Result<(Vec<chizu_core::Entity>, Vec<chizu_core::Edge>)> {
    let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let name = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if ext == "rs" {
        return rust::index_rust_file(file, repo_root);
    }

    if name == "mise.toml" {
        return mise::index_mise_file(file, repo_root);
    }

    if ext == "md" {
        let (mut entities, mut edges) = doc::index_doc_file(file)?;
        let (fm_entities, fm_edges) = frontmatter::index_frontmatter_file(file, repo_root)?;
        entities.extend(fm_entities);
        edges.extend(fm_edges);
        return Ok((entities, edges));
    }

    scanner::scan_file(file)
}
