use std::path::Path;

use crate::error::IndexError;

#[derive(Debug, Clone)]
pub struct MiseTask {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct MiseConfig {
    pub tasks: Vec<MiseTask>,
}

pub fn parse_mise_toml(path: &Path) -> Result<Option<MiseConfig>, IndexError> {
    let mise_path = path.join("mise.toml");
    if !mise_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&mise_path)?;
    let table: toml::Value =
        toml::from_str(&content).map_err(|e| IndexError::Parse(format!("mise.toml: {e}")))?;

    let mut tasks = Vec::new();

    if let Some(toml::Value::Table(tasks_table)) = table.get("tasks") {
        collect_tasks(tasks_table, "", &mut tasks);
    }

    Ok(Some(MiseConfig { tasks }))
}

fn collect_tasks(
    table: &toml::map::Map<String, toml::Value>,
    prefix: &str,
    tasks: &mut Vec<MiseTask>,
) {
    for (key, value) in table {
        let full_name = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}:{key}")
        };
        match value {
            // Simple string command: `task_name = "command"`
            toml::Value::String(_) => {
                tasks.push(MiseTask { name: full_name });
            }
            // Table with run key or nested tasks
            toml::Value::Table(inner) => {
                if inner.contains_key("run") || inner.contains_key("cmd") {
                    tasks.push(MiseTask { name: full_name });
                } else {
                    // Nested task group
                    collect_tasks(inner, &full_name, tasks);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("grakno_test_mise_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_missing_mise_toml() {
        let dir = test_dir("missing");
        let result = parse_mise_toml(&dir).unwrap();
        assert!(result.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_basic_mise_toml() {
        let dir = test_dir("basic");
        let content = r#"
[tasks]
build = "cargo build"
test = "cargo test"

[tasks.lint]
run = "cargo clippy"
"#;
        fs::write(dir.join("mise.toml"), content).unwrap();
        let config = parse_mise_toml(&dir).unwrap().unwrap();
        let names: Vec<_> = config.tasks.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"build"));
        assert!(names.contains(&"test"));
        assert!(names.contains(&"lint"));
        let _ = fs::remove_dir_all(&dir);
    }
}
