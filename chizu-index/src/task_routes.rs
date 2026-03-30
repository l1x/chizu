use chizu_core::{Entity, EntityKind, TaskRoute};

pub fn generate_task_routes(entity: &Entity) -> Vec<TaskRoute> {
    let mut routes = Vec::new();
    let name_lower = entity.name.to_lowercase();
    let path_lower = entity.path.as_ref().map(|p| p.to_lowercase());

    match entity.kind {
        EntityKind::Component => {
            routes.push(TaskRoute::new("understand", &entity.id, 80));
            routes.push(TaskRoute::new("architecture", &entity.id, 80));
            routes.push(TaskRoute::new("build", &entity.id, 70));
            routes.push(TaskRoute::new("implement", &entity.id, 70));
        }
        EntityKind::SourceUnit => {
            let is_mod_or_lib = path_lower
                .as_ref()
                .map(|p| p.ends_with("mod.rs") || p.ends_with("lib.rs"))
                .unwrap_or(false);
            let prio = if is_mod_or_lib { 60 } else { 30 };
            routes.push(TaskRoute::new("understand", &entity.id, prio));
            routes.push(TaskRoute::new("architecture", &entity.id, prio));
            routes.push(TaskRoute::new("debug", &entity.id, 50));
            routes.push(TaskRoute::new("fix", &entity.id, 50));
            routes.push(TaskRoute::new("build", &entity.id, 40));
            routes.push(TaskRoute::new("implement", &entity.id, 40));
        }
        EntityKind::Symbol => {
            routes.push(TaskRoute::new("build", &entity.id, 50));
            routes.push(TaskRoute::new("implement", &entity.id, 50));
        }
        EntityKind::Test => {
            routes.push(TaskRoute::new("test", &entity.id, 80));
            routes.push(TaskRoute::new("bench", &entity.id, 40));
            routes.push(TaskRoute::new("debug", &entity.id, 60));
            routes.push(TaskRoute::new("fix", &entity.id, 60));
        }
        EntityKind::Doc => {
            routes.push(TaskRoute::new("understand", &entity.id, 70));
            routes.push(TaskRoute::new("architecture", &entity.id, 70));
        }
        EntityKind::ContentPage => {
            routes.push(TaskRoute::new("understand", &entity.id, 60));
            routes.push(TaskRoute::new("build", &entity.id, 40));
        }
        EntityKind::Template => {
            routes.push(TaskRoute::new("build", &entity.id, 60));
            routes.push(TaskRoute::new("understand", &entity.id, 40));
        }
        EntityKind::Site => {
            routes.push(TaskRoute::new("understand", &entity.id, 70));
            routes.push(TaskRoute::new("deploy", &entity.id, 70));
            routes.push(TaskRoute::new("build", &entity.id, 60));
        }
        EntityKind::Migration => {
            routes.push(TaskRoute::new("build", &entity.id, 60));
            routes.push(TaskRoute::new("debug", &entity.id, 50));
        }
        EntityKind::Spec => {
            routes.push(TaskRoute::new("understand", &entity.id, 70));
            routes.push(TaskRoute::new("test", &entity.id, 60));
            routes.push(TaskRoute::new("debug", &entity.id, 50));
        }
        EntityKind::Workflow => {
            routes.push(TaskRoute::new("configure", &entity.id, 60));
            routes.push(TaskRoute::new("build", &entity.id, 40));
        }
        EntityKind::AgentConfig => {
            routes.push(TaskRoute::new("configure", &entity.id, 70));
            routes.push(TaskRoute::new("understand", &entity.id, 60));
        }
        EntityKind::Feature => {
            routes.push(TaskRoute::new("configure", &entity.id, 70));
            routes.push(TaskRoute::new("setup", &entity.id, 70));
        }
        EntityKind::Task => {
            if name_lower.contains("deploy")
                || name_lower.contains("release")
                || name_lower.contains("ci")
            {
                routes.push(TaskRoute::new("deploy", &entity.id, 80));
                routes.push(TaskRoute::new("release", &entity.id, 80));
            } else if name_lower.contains("test") {
                routes.push(TaskRoute::new("test", &entity.id, 70));
                routes.push(TaskRoute::new("bench", &entity.id, 40));
            } else if name_lower.contains("build") {
                routes.push(TaskRoute::new("build", &entity.id, 70));
                routes.push(TaskRoute::new("implement", &entity.id, 40));
            }
        }
        EntityKind::Bench => {
            routes.push(TaskRoute::new("test", &entity.id, 80));
            routes.push(TaskRoute::new("bench", &entity.id, 80));
        }
        EntityKind::Containerized => {
            routes.push(TaskRoute::new("deploy", &entity.id, 80));
        }
        EntityKind::InfraRoot => {
            routes.push(TaskRoute::new("deploy", &entity.id, 80));
            routes.push(TaskRoute::new("configure", &entity.id, 70));
        }
        EntityKind::Command => {
            routes.push(TaskRoute::new("deploy", &entity.id, 70));
            routes.push(TaskRoute::new("build", &entity.id, 50));
        }
        EntityKind::Repo | EntityKind::Directory => {}
    }

    // Cross-cutting: entities with "config" in name or path get configure/setup.
    let has_config = name_lower.contains("config")
        || path_lower
            .as_ref()
            .map(|p| p.contains("config"))
            .unwrap_or(false);
    if has_config {
        routes.push(TaskRoute::new("configure", &entity.id, 60));
        routes.push(TaskRoute::new("setup", &entity.id, 60));
    }

    routes
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{Entity, EntityKind};

    fn routes_for(entity: &Entity) -> Vec<(String, i32)> {
        generate_task_routes(entity)
            .into_iter()
            .map(|r| (r.task_name, r.priority))
            .collect()
    }

    #[test]
    fn test_component_routes() {
        let entity = Entity::new("component::cargo::.", EntityKind::Component, "root");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("understand".to_string(), 80)));
        assert!(routes.contains(&("architecture".to_string(), 80)));
        assert!(routes.contains(&("build".to_string(), 70)));
        assert!(routes.contains(&("implement".to_string(), 70)));
    }

    #[test]
    fn test_source_unit_mod_rs() {
        let entity = Entity::new("source_unit::src/lib.rs", EntityKind::SourceUnit, "lib")
            .with_path("src/lib.rs");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("understand".to_string(), 60)));
        assert!(routes.contains(&("architecture".to_string(), 60)));
    }

    #[test]
    fn test_source_unit_other() {
        let entity = Entity::new("source_unit::src/main.rs", EntityKind::SourceUnit, "main")
            .with_path("src/main.rs");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("understand".to_string(), 30)));
        assert!(routes.contains(&("architecture".to_string(), 30)));
    }

    #[test]
    fn test_test_routes() {
        let entity = Entity::new("test::src/lib.rs::it_works", EntityKind::Test, "it_works");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("test".to_string(), 80)));
        assert!(routes.contains(&("bench".to_string(), 40)));
        assert!(routes.contains(&("debug".to_string(), 60)));
        assert!(routes.contains(&("fix".to_string(), 60)));
    }

    #[test]
    fn test_symbol_routes() {
        let entity = Entity::new("symbol::src/lib.rs::foo", EntityKind::Symbol, "foo");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("build".to_string(), 50)));
        assert!(routes.contains(&("implement".to_string(), 50)));
    }

    #[test]
    fn test_config_cross_cutting() {
        let entity = Entity::new("symbol::src/config.rs::load", EntityKind::Symbol, "load")
            .with_path("src/config.rs");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("configure".to_string(), 60)));
        assert!(routes.contains(&("setup".to_string(), 60)));
    }

    #[test]
    fn test_task_deploy() {
        let entity = Entity::new("task::mise.toml::deploy", EntityKind::Task, "deploy");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("deploy".to_string(), 80)));
        assert!(routes.contains(&("release".to_string(), 80)));
    }

    #[test]
    fn test_task_test() {
        let entity = Entity::new("task::mise.toml::test", EntityKind::Task, "test");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("test".to_string(), 70)));
        assert!(routes.contains(&("bench".to_string(), 40)));
    }

    #[test]
    fn test_doc_routes() {
        let entity = Entity::new("doc::README.md", EntityKind::Doc, "README");
        let routes = routes_for(&entity);
        assert!(routes.contains(&("understand".to_string(), 70)));
        assert!(routes.contains(&("architecture".to_string(), 70)));
    }
}
