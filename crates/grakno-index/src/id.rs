pub fn repo_id(name: &str) -> String {
    format!("repo::{name}")
}

pub fn component_id(crate_name: &str) -> String {
    format!("component::{crate_name}")
}

pub fn source_unit_id(crate_name: &str, path: &str) -> String {
    format!("source_unit::{crate_name}::{path}")
}

pub fn symbol_id(crate_name: &str, symbol_name: &str) -> String {
    format!("symbol::{crate_name}::{symbol_name}")
}

pub fn test_id(crate_name: &str, fn_name: &str) -> String {
    format!("test::{crate_name}::{fn_name}")
}

pub fn bench_id(crate_name: &str, fn_name: &str) -> String {
    format!("bench::{crate_name}::{fn_name}")
}

pub fn feature_id(crate_name: &str, feature_name: &str) -> String {
    format!("feature::{crate_name}::{feature_name}")
}

pub fn doc_id(crate_name: &str, path: &str) -> String {
    format!("doc::{crate_name}::{path}")
}

pub fn task_id(task_name: &str) -> String {
    format!("task::{task_name}")
}

pub fn migration_id(component_name: &str, path: &str) -> String {
    format!("migration::{component_name}::{path}")
}

pub fn spec_id(component_name: &str, path: &str) -> String {
    format!("spec::{component_name}::{path}")
}

pub fn workflow_id(path: &str) -> String {
    format!("workflow::{path}")
}

pub fn agent_config_id(path: &str) -> String {
    format!("agent_config::{path}")
}

pub fn template_id(path: &str) -> String {
    format!("template::{path}")
}

pub fn infra_root_id(path: &str) -> String {
    format!("infra_root::{path}")
}

pub fn containerized_id(path: &str) -> String {
    format!("containerized::{path}")
}

pub fn command_id(path: &str) -> String {
    format!("command::{path}")
}

pub fn site_id(name: &str) -> String {
    format!("site::{name}")
}

pub fn content_page_id(site_name: &str, path: &str) -> String {
    format!("content_page::{site_name}::{path}")
}

/// Generic file entity ID (for non-Cargo projects)
pub fn file_entity_id(path: &str) -> String {
    format!("file::{path}")
}

/// Symbol within a specific file (for non-Cargo projects)
pub fn symbol_in_file(path: &str, symbol_name: &str) -> String {
    format!("symbol::{path}::{symbol_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_formats() {
        assert_eq!(repo_id("grakno"), "repo::grakno");
        assert_eq!(component_id("grakno-core"), "component::grakno-core");
        assert_eq!(
            source_unit_id("grakno-core", "crates/grakno-core/src/lib.rs"),
            "source_unit::grakno-core::crates/grakno-core/src/lib.rs"
        );
        assert_eq!(
            symbol_id("grakno-core", "Store"),
            "symbol::grakno-core::Store"
        );
        assert_eq!(
            test_id("grakno-core", "test_foo"),
            "test::grakno-core::test_foo"
        );
        assert_eq!(
            bench_id("grakno-core", "bench_bar"),
            "bench::grakno-core::bench_bar"
        );
        assert_eq!(
            feature_id("grakno-core", "default"),
            "feature::grakno-core::default"
        );
        assert_eq!(
            doc_id("grakno-core", "README.md"),
            "doc::grakno-core::README.md"
        );
        assert_eq!(task_id("build"), "task::build");
        assert_eq!(
            migration_id("myapp", "migrations/001.sql"),
            "migration::myapp::migrations/001.sql"
        );
        assert_eq!(
            spec_id("myapp", "specs/main.tla"),
            "spec::myapp::specs/main.tla"
        );
        assert_eq!(
            workflow_id("workflows/deploy.toml"),
            "workflow::workflows/deploy.toml"
        );
        assert_eq!(agent_config_id("CLAUDE.md"), "agent_config::CLAUDE.md");
        assert_eq!(
            template_id("templates/base.html"),
            "template::templates/base.html"
        );
        assert_eq!(infra_root_id("infra/prod"), "infra_root::infra/prod");
        assert_eq!(containerized_id("Dockerfile"), "containerized::Dockerfile");
        assert_eq!(
            command_id("playbooks/deploy.yml"),
            "command::playbooks/deploy.yml"
        );
        assert_eq!(site_id("dev.l1x.be"), "site::dev.l1x.be");
        assert_eq!(
            content_page_id("dev.l1x.be", "content/blog/post.md"),
            "content_page::dev.l1x.be::content/blog/post.md"
        );
    }
}
