pub mod error;
pub mod model;
pub mod store;

pub use error::{GraknoError, Result};
pub use store::stats::GraphStats;
pub use store::Store;

#[cfg(test)]
mod tests {
    use super::*;
    use model::*;

    #[test]
    fn graph_round_trip() {
        let store = Store::open_in_memory().unwrap();

        // Insert a component entity
        let comp = Entity {
            id: "component::harrow-core".to_string(),
            kind: EntityKind::Component,
            name: "harrow-core".to_string(),
            component_id: None,
            path: Some("crates/harrow-core".to_string()),
            language: Some("rust".to_string()),
            line_start: None,
            line_end: None,
            visibility: Some("pub".to_string()),
            exported: true,
        };
        store.insert_entity(&comp).unwrap();

        // Insert a symbol inside it
        let sym = Entity {
            id: "symbol::harrow-core::Engine".to_string(),
            kind: EntityKind::Symbol,
            name: "Engine".to_string(),
            component_id: Some("component::harrow-core".to_string()),
            path: Some("crates/harrow-core/src/engine.rs".to_string()),
            language: Some("rust".to_string()),
            line_start: Some(15),
            line_end: Some(120),
            visibility: Some("pub".to_string()),
            exported: true,
        };
        store.insert_entity(&sym).unwrap();

        // Connect them
        let edge = Edge {
            src_id: "component::harrow-core".to_string(),
            rel: EdgeKind::Contains,
            dst_id: "symbol::harrow-core::Engine".to_string(),
            provenance_path: Some("Cargo.toml".to_string()),
            provenance_line: None,
        };
        store.insert_edge(&edge).unwrap();

        // Track the file
        let file = FileRecord {
            path: "crates/harrow-core/src/engine.rs".to_string(),
            component_id: Some("component::harrow-core".to_string()),
            kind: "rust".to_string(),
            hash: "sha256:abc123".to_string(),
            indexed: true,
            ignore_reason: None,
        };
        store.insert_file(&file).unwrap();

        // Add a summary
        let summary = Summary {
            entity_id: "symbol::harrow-core::Engine".to_string(),
            short_summary: "Core execution engine for Harrow".to_string(),
            detailed_summary: None,
            keywords: vec!["engine".to_string(), "execution".to_string()],
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            source_hash: None,
        };
        store.upsert_summary(&summary).unwrap();

        // Add a task route
        let route = TaskRoute {
            task_name: "build".to_string(),
            entity_id: "component::harrow-core".to_string(),
            priority: 10,
        };
        store.insert_task_route(&route).unwrap();

        // Verify graph traversal
        let outgoing = store.edges_from("component::harrow-core").unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].dst_id, "symbol::harrow-core::Engine");

        let incoming = store.edges_to("symbol::harrow-core::Engine").unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].src_id, "component::harrow-core");

        // Verify entity lookup
        let entities = store
            .list_entities_by_component("component::harrow-core")
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Engine");

        // Verify summary
        let got_summary = store.get_summary("symbol::harrow-core::Engine").unwrap();
        assert_eq!(got_summary.keywords, vec!["engine", "execution"]);

        // Verify task routes
        let routes = store.routes_for_task("build").unwrap();
        assert_eq!(routes.len(), 1);

        // Verify schema version
        assert_eq!(store.schema_version().unwrap(), Some(4));
    }

    #[cfg(feature = "grafeo")]
    #[test]
    fn graph_round_trip_grafeo() {
        let store = Store::open_grafeo_in_memory().unwrap();

        // Insert a component entity
        let comp = Entity {
            id: "component::harrow-core".to_string(),
            kind: EntityKind::Component,
            name: "harrow-core".to_string(),
            component_id: None,
            path: Some("crates/harrow-core".to_string()),
            language: Some("rust".to_string()),
            line_start: None,
            line_end: None,
            visibility: Some("pub".to_string()),
            exported: true,
        };
        store.insert_entity(&comp).unwrap();

        // Insert a symbol inside it
        let sym = Entity {
            id: "symbol::harrow-core::Engine".to_string(),
            kind: EntityKind::Symbol,
            name: "Engine".to_string(),
            component_id: Some("component::harrow-core".to_string()),
            path: Some("crates/harrow-core/src/engine.rs".to_string()),
            language: Some("rust".to_string()),
            line_start: Some(15),
            line_end: Some(120),
            visibility: Some("pub".to_string()),
            exported: true,
        };
        store.insert_entity(&sym).unwrap();

        // Connect them
        let edge = Edge {
            src_id: "component::harrow-core".to_string(),
            rel: EdgeKind::Contains,
            dst_id: "symbol::harrow-core::Engine".to_string(),
            provenance_path: Some("Cargo.toml".to_string()),
            provenance_line: None,
        };
        store.insert_edge(&edge).unwrap();

        // Track the file
        let file = FileRecord {
            path: "crates/harrow-core/src/engine.rs".to_string(),
            component_id: Some("component::harrow-core".to_string()),
            kind: "rust".to_string(),
            hash: "sha256:abc123".to_string(),
            indexed: true,
            ignore_reason: None,
        };
        store.insert_file(&file).unwrap();

        // Add a summary
        let summary = Summary {
            entity_id: "symbol::harrow-core::Engine".to_string(),
            short_summary: "Core execution engine for Harrow".to_string(),
            detailed_summary: None,
            keywords: vec!["engine".to_string(), "execution".to_string()],
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            source_hash: None,
        };
        store.upsert_summary(&summary).unwrap();

        // Add a task route
        let route = TaskRoute {
            task_name: "build".to_string(),
            entity_id: "component::harrow-core".to_string(),
            priority: 10,
        };
        store.insert_task_route(&route).unwrap();

        // Verify graph traversal
        let outgoing = store.edges_from("component::harrow-core").unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].dst_id, "symbol::harrow-core::Engine");

        let incoming = store.edges_to("symbol::harrow-core::Engine").unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].src_id, "component::harrow-core");

        // Verify entity lookup
        let entities = store
            .list_entities_by_component("component::harrow-core")
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Engine");

        // Verify summary
        let got_summary = store.get_summary("symbol::harrow-core::Engine").unwrap();
        assert_eq!(got_summary.keywords, vec!["engine", "execution"]);

        // Verify task routes
        let routes = store.routes_for_task("build").unwrap();
        assert_eq!(routes.len(), 1);

        // Verify schema version (grafeo has no schema versioning)
        assert_eq!(store.schema_version().unwrap(), None);
    }
}
