use std::path::{Path, PathBuf};

use chizu_core::{ComponentId, Config};
use globset::GlobSet;
use ignore::WalkBuilder;

use crate::error::Result;

/// A file discovered during repository traversal.
#[derive(Debug, Clone)]
pub struct WalkedFile {
    /// Repository-relative path.
    pub path: PathBuf,
    /// BLAKE3 hash of file content (hex).
    pub hash: String,
    /// The component this file belongs to (if determined).
    pub component_id: Option<ComponentId>,
}

/// File walker that respects `.gitignore` and config exclude patterns.
pub struct FileWalker {
    repo_root: PathBuf,
    exclude_matcher: Option<GlobSet>,
}

impl FileWalker {
    /// Create a new walker for the given repository root and config.
    pub fn new(repo_root: &Path, config: &Config) -> Result<Self> {
        let exclude_matcher = if config.index.exclude_patterns.is_empty() {
            None
        } else {
            let mut builder = globset::GlobSetBuilder::new();
            for pattern in &config.index.exclude_patterns {
                builder.add(globset::Glob::new(pattern)?);
            }
            Some(builder.build()?)
        };

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            exclude_matcher,
        })
    }

    /// Walk the repository and return all non-excluded files with hashes.
    pub fn walk(&self) -> Result<Vec<WalkedFile>> {
        let mut files = Vec::new();
        let walker = WalkBuilder::new(&self.repo_root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .build();

        for entry in walker {
            let entry = entry.map_err(|e| crate::error::IndexError::Walk(e.to_string()))?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let rel_path = match path.strip_prefix(&self.repo_root) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let rel_str = rel_path.to_string_lossy();

            if self.is_excluded(&rel_str) {
                continue;
            }

            let hash = {
                let file = std::fs::File::open(path)?;
                let mut reader = std::io::BufReader::new(file);
                let mut hasher = blake3::Hasher::new();
                std::io::copy(&mut reader, &mut hasher)?;
                hasher.finalize().to_hex().to_string()
            };

            files.push(WalkedFile {
                path: rel_path.to_path_buf(),
                hash,
                component_id: None,
            });
        }

        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    fn is_excluded(&self, rel_path: &str) -> bool {
        self.exclude_matcher
            .as_ref()
            .map(|m| m.is_match(rel_path))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn walk_respects_gitignore_and_excludes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create structure
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();

        fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(root.join("target/debug/foo"), b"binary").unwrap();
        fs::write(root.join("logs/app.log"), b"log data").unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();

        let mut config = Config::default();
        config.index.exclude_patterns = vec!["**/*.log".to_string()];

        let walker = FileWalker::new(root, &config).unwrap();
        let files = walker.walk().unwrap();

        let paths: Vec<_> = files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();

        assert!(paths.contains(&"src/main.rs".to_string()));
        assert!(
            !paths.contains(&"target/debug/foo".to_string()),
            "should respect .gitignore"
        );
        assert!(
            !paths.contains(&"logs/app.log".to_string()),
            "should respect exclude_patterns"
        );

        // Verify hash
        let main_rs = files
            .iter()
            .find(|f| f.path == Path::new("src/main.rs"))
            .unwrap();
        let expected_hash = blake3::hash(b"fn main() {}").to_hex().to_string();
        assert_eq!(main_rs.hash, expected_hash);
    }
}
