use std::path::Path;

use chizu_core::{Edge, EdgeKind, Entity, EntityKind, entity_id};

use crate::error::Result;
use crate::walk::WalkedFile;

/// Facts extracted from site detection.
#[derive(Debug, Default)]
pub struct SiteFacts {
    pub entities: Vec<Entity>,
    pub edges: Vec<Edge>,
}

/// Detect site roots and emit site entities with edges to related content.
///
/// V1: scans walked files for site root markers, then matches related files
/// by path proximity. No store query required.
pub fn index_sites(repo_root: &Path, files: &[WalkedFile]) -> Result<SiteFacts> {
    let mut facts = SiteFacts::default();
    let site_roots = detect_site_roots(files, repo_root);

    for (site_path, site_name) in site_roots {
        let site_id = entity_id("site", &site_path);
        facts.entities.push(
            Entity::new(&site_id, EntityKind::Site, &site_name)
                .with_path(&site_path)
                .with_exported(true),
        );

        for file in files {
            let rel = &file.path;
            if site_path != "." && !rel.starts_with(&site_path) {
                continue;
            }

            // Content pages under site root
            if is_content_page(rel) {
                let page_id = entity_id("content_page", &rel.to_string_lossy());
                facts.edges.push(Edge::new(&site_id, EdgeKind::Contains, &page_id));
            }

            // Templates under site root
            if is_template(rel) {
                let tpl_id = entity_id("template", &rel.to_string_lossy());
                facts.edges.push(Edge::new(&tpl_id, EdgeKind::Renders, &site_id));
            }

            // Infra roots under site root or at repo root
            if is_infra_root(rel) {
                let infra_id = entity_id("infra_root", &rel.to_string_lossy());
                facts.edges.push(Edge::new(&site_id, EdgeKind::Deploys, &infra_id));
            }
        }
    }

    Ok(facts)
}

fn detect_site_roots(files: &[WalkedFile], repo_root: &Path) -> Vec<(String, String)> {
    let mut roots = Vec::new();

    for file in files {
        let name = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let parent = file.path.parent().unwrap_or(Path::new(""));
        let parent_str = if parent.as_os_str().is_empty() {
            ".".to_string()
        } else {
            parent.to_string_lossy().to_string()
        };

        if name == "site.toml" {
            roots.push((parent_str.clone(), "site".to_string()));
            continue;
        }

        if name.starts_with("astro.config.") {
            roots.push((parent_str.clone(), "astro-site".to_string()));
            continue;
        }

        if name == "hugo.toml" {
            roots.push((parent_str.clone(), "hugo-site".to_string()));
            continue;
        }

        if name == "config.toml" && parent.as_os_str().is_empty() {
            // Could be Hugo — check for Hugo-specific keys
            let abs_path = repo_root.join(&file.path);
            if let Ok(content) = std::fs::read_to_string(&abs_path) {
                if content.contains("baseURL") || content.contains("theme") {
                    roots.push((parent_str, "hugo-site".to_string()));
                }
            }
        }
    }

    roots
}

fn is_content_page(path: &Path) -> bool {
    let in_dir = path.components().any(|c| {
        let s = c.as_os_str();
        s == "content" || s == "pages" || s == "blog"
    });
    in_dir && path.extension().and_then(|e| e.to_str()) == Some("md")
}

fn is_template(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let in_dir = path.components().any(|c| {
        let s = c.as_os_str();
        s == "templates" || s == "layouts"
    });
    in_dir
        && (ext == "html"
            || ext == "astro"
            || ext == "hbs"
            || ext == "svelte"
            || ext == "vue"
            || ext == "njk"
            || ext == "tera")
}

fn is_infra_root(path: &Path) -> bool {
    path.file_name() == Some(std::ffi::OsStr::new("main.tf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn site_adapter_detects_astro_site() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(root.join("astro.config.mjs"), "export default {};").unwrap();
        fs::create_dir_all(root.join("src/pages")).unwrap();
        fs::write(root.join("src/pages/index.md"), "+++\n+++\n").unwrap();
        fs::create_dir_all(root.join("src/layouts")).unwrap();
        fs::write(root.join("src/layouts/Base.astro"), "").unwrap();
        fs::create_dir_all(root.join("infra")).unwrap();
        fs::write(root.join("infra/main.tf"), "").unwrap();

        let config = crate::walk::FileWalker::new(root, &chizu_core::Config::default())
            .unwrap()
            .walk()
            .unwrap();

        let facts = index_sites(root, &config).unwrap();

        assert!(facts.entities.iter().any(|e| e.id == "site::."));
        assert!(facts.edges.iter().any(|e| {
            e.src_id == "site::." && e.rel == EdgeKind::Contains && e.dst_id == "content_page::src/pages/index.md"
        }));
        assert!(facts.edges.iter().any(|e| {
            e.src_id == "template::src/layouts/Base.astro" && e.rel == EdgeKind::Renders && e.dst_id == "site::."
        }));
        assert!(facts.edges.iter().any(|e| {
            e.src_id == "site::." && e.rel == EdgeKind::Deploys && e.dst_id == "infra_root::infra/main.tf"
        }));
    }
}
