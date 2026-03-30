use chizu_core::{Edge, Entity, EntityKind, entity_id};

use crate::error::Result;
use crate::walk::WalkedFile;

/// Index a markdown file with frontmatter as a content page.
pub fn index_frontmatter_file(file: &WalkedFile, repo_root: &std::path::Path) -> Result<(Vec<Entity>, Vec<Edge>)> {
    let path_str = file.path.to_string_lossy();

    // Only process .md files in content directories
    if !super::is_content_dir(&file.path) || file.path.extension().and_then(|e| e.to_str()) != Some("md") {
        return Ok((Vec::new(), Vec::new()));
    }

    let abs_path = repo_root.join(&file.path);
    let content = std::fs::read_to_string(&abs_path)?;

    let has_frontmatter = content.starts_with("+++") || content.starts_with("---");
    if !has_frontmatter {
        return Ok((Vec::new(), Vec::new()));
    }

    let id = entity_id("content_page", &path_str);
    let name = file
        .path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("page");

    let entity = Entity::new(&id, EntityKind::ContentPage, name)
        .with_path(path_str.as_ref())
        .with_exported(true);

    Ok((vec![entity], Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn frontmatter_adapter_extracts_content_page() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("content/blog")).unwrap();
        fs::write(
            root.join("content/blog/post.md"),
            "+++\ntitle = \"Hello\"\n+++\n\nContent here.\n",
        )
        .unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("content/blog/post.md"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (entities, _) = index_frontmatter_file(&file, root).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id, "content_page::content/blog/post.md");
        assert_eq!(entities[0].kind, EntityKind::ContentPage);
    }

    #[test]
    fn frontmatter_adapter_skips_plain_md() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("content")).unwrap();
        fs::write(root.join("content/plain.md"), "No frontmatter here.\n").unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("content/plain.md"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (entities, _) = index_frontmatter_file(&file, root).unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn frontmatter_adapter_skips_non_content_md() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("README.md"), "+++\n+++\n").unwrap();

        let file = WalkedFile {
            path: std::path::PathBuf::from("README.md"),
            hash: "abc".to_string(),
            component_id: None,
        };
        let (entities, _) = index_frontmatter_file(&file, root).unwrap();
        assert!(entities.is_empty());
    }
}
