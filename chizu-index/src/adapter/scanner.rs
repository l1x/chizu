use std::sync::OnceLock;

use chizu_core::{Edge, EdgeKind, Entity, EntityKind, entity_id};
use globset::GlobSet;

use crate::error::Result;
use crate::walk::WalkedFile;

struct ScanRule {
    glob: &'static str,
    entity_kind: EntityKind,
    id_prefix: &'static str,
    edge: Option<(EdgeKind, &'static str)>,
}

const RULES: &[ScanRule] = &[
    ScanRule {
        glob: "**/main.tf",
        entity_kind: EntityKind::InfraRoot,
        id_prefix: "infra_root",
        edge: None,
    },
    ScanRule {
        glob: "Dockerfile*",
        entity_kind: EntityKind::Containerized,
        id_prefix: "containerized",
        edge: None,
    },
    ScanRule {
        glob: "docker-compose*.yml",
        entity_kind: EntityKind::Containerized,
        id_prefix: "containerized",
        edge: None,
    },
    ScanRule {
        glob: "**/playbooks/*.yml",
        entity_kind: EntityKind::Command,
        id_prefix: "command",
        edge: None,
    },
    ScanRule {
        glob: "**/migrations/*.sql",
        entity_kind: EntityKind::Migration,
        id_prefix: "migration",
        edge: Some((EdgeKind::Migrates, "repo::.")),
    },
    ScanRule {
        glob: "**/*.tla",
        entity_kind: EntityKind::Spec,
        id_prefix: "spec",
        edge: Some((EdgeKind::Specifies, "repo::.")),
    },
    ScanRule {
        glob: ".github/workflows/*.{yml,yaml}",
        entity_kind: EntityKind::Workflow,
        id_prefix: "workflow",
        edge: None,
    },
    ScanRule {
        glob: "**/.circleci/*.{yml,yaml}",
        entity_kind: EntityKind::Workflow,
        id_prefix: "workflow",
        edge: None,
    },
    ScanRule {
        glob: "**/workflows/*.{toml,yml,yaml}",
        entity_kind: EntityKind::Workflow,
        id_prefix: "workflow",
        edge: None,
    },
    ScanRule {
        glob: "CLAUDE.md",
        entity_kind: EntityKind::AgentConfig,
        id_prefix: "agent_config",
        edge: None, // handled specially below
    },
    ScanRule {
        glob: "AGENTS.md",
        entity_kind: EntityKind::AgentConfig,
        id_prefix: "agent_config",
        edge: None,
    },
    ScanRule {
        glob: "SKILL.md",
        entity_kind: EntityKind::AgentConfig,
        id_prefix: "agent_config",
        edge: None,
    },
    ScanRule {
        glob: "templates/**/*.html",
        entity_kind: EntityKind::Template,
        id_prefix: "template",
        edge: None,
    },
    ScanRule {
        glob: "layouts/**/*.html",
        entity_kind: EntityKind::Template,
        id_prefix: "template",
        edge: None,
    },
    ScanRule {
        glob: "**/*.astro",
        entity_kind: EntityKind::Template,
        id_prefix: "template",
        edge: None,
    },
    ScanRule {
        glob: "**/*.hbs",
        entity_kind: EntityKind::Template,
        id_prefix: "template",
        edge: None,
    },
];

fn matchers() -> &'static Vec<(GlobSet, &'static ScanRule)> {
    static MATCHERS: OnceLock<Vec<(GlobSet, &'static ScanRule)>> = OnceLock::new();
    MATCHERS.get_or_init(|| {
        RULES
            .iter()
            .map(|rule| {
                let glob = globset::Glob::new(rule.glob).expect("invalid glob in scan rule");
                let matcher = globset::GlobSetBuilder::new()
                    .add(glob)
                    .build()
                    .expect("failed to build globset");
                (matcher, rule)
            })
            .collect()
    })
}

/// Scan a file against declarative rules and emit entities/edges.
pub fn scan_file(file: &WalkedFile) -> Result<(Vec<Entity>, Vec<Edge>)> {
    let mut entities = Vec::new();
    let mut edges = Vec::new();
    let path_str = file.path.to_string_lossy();

    for (matcher, rule) in matchers() {
        if !matcher.is_match(&*path_str) {
            continue;
        }

        let name = file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&path_str);
        let id = entity_id(rule.id_prefix, &path_str);
        let mut entity = Entity::new(&id, rule.entity_kind, name)
            .with_path(path_str.as_ref())
            .with_exported(true);
        if let Some(component_id) = file.component_id.as_ref() {
            entity = entity.with_component(component_id.clone());
        }
        entities.push(entity);

        if let Some((rel, src)) = rule.edge {
            edges.push(Edge::new(src, rel, &id));
        }

        // Agent configs get a configured_by edge from their enclosing component
        // or from the repo root if outside any component.
        if rule.entity_kind == EntityKind::AgentConfig {
            let src = file
                .component_id
                .as_ref()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "repo::.".to_string());
            edges.push(Edge::new(&src, EdgeKind::ConfiguredBy, &id));
        }

        break; // first match wins
    }

    Ok((entities, edges))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::ComponentId;
    use std::path::PathBuf;

    fn walked(path: &str) -> WalkedFile {
        WalkedFile {
            path: PathBuf::from(path),
            hash: "abc".to_string(),
            component_id: None,
        }
    }

    fn walked_with_component(path: &str, component_id: ComponentId) -> WalkedFile {
        WalkedFile {
            path: PathBuf::from(path),
            hash: "abc".to_string(),
            component_id: Some(component_id),
        }
    }

    #[test]
    fn scanner_infra_root() {
        let (entities, edges) = scan_file(&walked("modules/vpc/main.tf")).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id, "infra_root::modules/vpc/main.tf");
        assert_eq!(entities[0].kind, EntityKind::InfraRoot);
        assert!(edges.is_empty());
    }

    #[test]
    fn scanner_containerized() {
        let (entities, _) = scan_file(&walked("Dockerfile")).unwrap();
        assert_eq!(entities[0].id, "containerized::Dockerfile");
        assert_eq!(entities[0].kind, EntityKind::Containerized);
    }

    #[test]
    fn scanner_command() {
        let (entities, _) = scan_file(&walked("playbooks/deploy.yml")).unwrap();
        assert_eq!(entities[0].kind, EntityKind::Command);
    }

    #[test]
    fn scanner_migration_with_edge() {
        let (entities, edges) = scan_file(&walked("migrations/001_init.sql")).unwrap();
        assert_eq!(entities[0].kind, EntityKind::Migration);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].src_id, "repo::.");
        assert_eq!(edges[0].rel, EdgeKind::Migrates);
    }

    #[test]
    fn scanner_spec_with_edge() {
        let (entities, edges) = scan_file(&walked("specs/system.tla")).unwrap();
        assert_eq!(entities[0].kind, EntityKind::Spec);
        assert_eq!(edges[0].rel, EdgeKind::Specifies);
    }

    #[test]
    fn scanner_workflow() {
        let (entities, _) = scan_file(&walked(".github/workflows/ci.yml")).unwrap();
        assert_eq!(entities[0].kind, EntityKind::Workflow);
    }

    #[test]
    fn scanner_agent_config_from_repo() {
        let (entities, edges) = scan_file(&walked("AGENTS.md")).unwrap();
        assert_eq!(entities[0].kind, EntityKind::AgentConfig);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].src_id, "repo::.");
        assert_eq!(edges[0].rel, EdgeKind::ConfiguredBy);
    }

    #[test]
    fn scanner_agent_config_from_component() {
        let file = walked_with_component("AGENTS.md", ComponentId::new("cargo", "."));
        let (_, edges) = scan_file(&file).unwrap();
        assert_eq!(edges[0].src_id, "component::cargo::.");
    }

    #[test]
    fn scanner_template() {
        let (entities, _) = scan_file(&walked("templates/base.html")).unwrap();
        assert_eq!(entities[0].kind, EntityKind::Template);
    }

    #[test]
    fn scanner_no_match() {
        let (entities, edges) = scan_file(&walked("src/main.rs")).unwrap();
        assert!(entities.is_empty());
        assert!(edges.is_empty());
    }
}
