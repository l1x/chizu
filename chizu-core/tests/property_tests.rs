//! Property-based tests for chizu-core using bolero.
//!
//! These tests verify invariants that must hold for *all* inputs,
//! not just hand-picked examples.

use bolero::check;
use chizu_core::{
    ComponentId, Config, Edge, EdgeKind, Entity, EntityKind, FileKind, FileRecord, Store, Summary,
    TaskRoute, Visibility,
};

// ── Variant tables ───────────────────────────────────────────────────

const ALL_ENTITY_KINDS: [EntityKind; 20] = [
    EntityKind::Repo,
    EntityKind::Directory,
    EntityKind::Component,
    EntityKind::SourceUnit,
    EntityKind::Symbol,
    EntityKind::Doc,
    EntityKind::Test,
    EntityKind::Bench,
    EntityKind::Task,
    EntityKind::Feature,
    EntityKind::Containerized,
    EntityKind::InfraRoot,
    EntityKind::Command,
    EntityKind::ContentPage,
    EntityKind::Template,
    EntityKind::Site,
    EntityKind::Migration,
    EntityKind::Spec,
    EntityKind::Workflow,
    EntityKind::AgentConfig,
];

const ALL_EDGE_KINDS: [EdgeKind; 18] = [
    EdgeKind::Contains,
    EdgeKind::Defines,
    EdgeKind::DependsOn,
    EdgeKind::Reexports,
    EdgeKind::DocumentedBy,
    EdgeKind::TestedBy,
    EdgeKind::BenchmarkedBy,
    EdgeKind::RelatedTo,
    EdgeKind::ConfiguredBy,
    EdgeKind::Builds,
    EdgeKind::Deploys,
    EdgeKind::Implements,
    EdgeKind::OwnsTask,
    EdgeKind::DeclaresFeature,
    EdgeKind::FeatureEnables,
    EdgeKind::Migrates,
    EdgeKind::Specifies,
    EdgeKind::Renders,
];

const ALL_VISIBILITIES: [Visibility; 4] = [
    Visibility::Public,
    Visibility::Private,
    Visibility::Protected,
    Visibility::Internal,
];

const ALL_FILE_KINDS: [FileKind; 10] = [
    FileKind::Source,
    FileKind::Doc,
    FileKind::Config,
    FileKind::Build,
    FileKind::Binary,
    FileKind::Data,
    FileKind::Template,
    FileKind::Migration,
    FileKind::Workflow,
    FileKind::Other,
];

// ── EntityKind roundtrip properties ──────────────────────────────────

#[test]
fn entity_kind_display_parse_roundtrip() {
    check!()
        .with_generator(0..ALL_ENTITY_KINDS.len())
        .for_each(|idx: &usize| {
            let kind = ALL_ENTITY_KINDS[*idx];
            let displayed = kind.to_string();
            let parsed: EntityKind = displayed.parse().unwrap();
            assert_eq!(kind, parsed);
        });
}

#[test]
fn entity_kind_serde_roundtrip() {
    check!()
        .with_generator(0..ALL_ENTITY_KINDS.len())
        .for_each(|idx: &usize| {
            let kind = ALL_ENTITY_KINDS[*idx];
            let json = serde_json::to_string(&kind).unwrap();
            let back: EntityKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        });
}

// ── EdgeKind roundtrip properties ────────────────────────────────────

#[test]
fn edge_kind_display_parse_roundtrip() {
    check!()
        .with_generator(0..ALL_EDGE_KINDS.len())
        .for_each(|idx: &usize| {
            let kind = ALL_EDGE_KINDS[*idx];
            let displayed = kind.to_string();
            let parsed: EdgeKind = displayed.parse().unwrap();
            assert_eq!(kind, parsed);
        });
}

#[test]
fn edge_kind_serde_roundtrip() {
    check!()
        .with_generator(0..ALL_EDGE_KINDS.len())
        .for_each(|idx: &usize| {
            let kind = ALL_EDGE_KINDS[*idx];
            let json = serde_json::to_string(&kind).unwrap();
            let back: EdgeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
        });
}

// ── Visibility roundtrip ─────────────────────────────────────────────

#[test]
fn visibility_display_parse_roundtrip() {
    check!()
        .with_generator(0..ALL_VISIBILITIES.len())
        .for_each(|idx: &usize| {
            let vis = ALL_VISIBILITIES[*idx];
            let displayed = vis.to_string();
            let parsed: Visibility = displayed.parse().unwrap();
            assert_eq!(vis, parsed);
        });
}

// ── FileKind roundtrip ───────────────────────────────────────────────

#[test]
fn file_kind_display_parse_roundtrip() {
    check!()
        .with_generator(0..ALL_FILE_KINDS.len())
        .for_each(|idx: &usize| {
            let kind = ALL_FILE_KINDS[*idx];
            let displayed = kind.to_string();
            let parsed: FileKind = displayed.parse().unwrap();
            assert_eq!(kind, parsed);
        });
}

// ── ComponentId properties ───────────────────────────────────────────

#[test]
fn component_id_new_always_parseable() {
    check!()
        .with_type::<(String, String)>()
        .for_each(|(ecosystem, path)| {
            let id = ComponentId::new(ecosystem, path);
            let parsed = ComponentId::parse(id.as_str());
            assert!(parsed.is_some(), "ComponentId::new result must be parseable");
            assert_eq!(parsed.unwrap(), id);
        });
}

#[test]
fn component_id_ecosystem_extraction() {
    check!()
        .with_type::<(String, String)>()
        .for_each(|(ecosystem, path)| {
            let id = ComponentId::new(ecosystem, path);
            // Ecosystem extraction works when ecosystem contains no `::`
            if !ecosystem.contains("::") {
                assert_eq!(id.ecosystem(), Some(ecosystem.as_str()));
            }
        });
}

#[test]
fn component_id_display_starts_with_prefix() {
    check!()
        .with_type::<(String, String)>()
        .for_each(|(ecosystem, path)| {
            let id = ComponentId::new(ecosystem, path);
            assert!(id.to_string().starts_with("component::"));
        });
}

#[test]
fn component_id_serde_roundtrip() {
    check!()
        .with_type::<(String, String)>()
        .for_each(|(ecosystem, path)| {
            let id = ComponentId::new(ecosystem, path);
            let json = serde_json::to_string(&id).unwrap();
            let back: ComponentId = serde_json::from_str(&json).unwrap();
            assert_eq!(id, back);
        });
}

// ── Entity JSON roundtrip ────────────────────────────────────────────

#[test]
fn entity_json_roundtrip() {
    check!()
        .with_generator((
            bolero::generator::produce::<String>(),
            0..ALL_ENTITY_KINDS.len(),
            bolero::generator::produce::<String>(),
            bolero::generator::produce::<bool>(),
        ))
        .for_each(|(id, kind_idx, name, exported)| {
            let kind = ALL_ENTITY_KINDS[*kind_idx];
            let entity = Entity::new(id.clone(), kind, name.clone()).with_exported(*exported);
            let json = serde_json::to_string(&entity).unwrap();
            let back: Entity = serde_json::from_str(&json).unwrap();
            assert_eq!(entity.id, back.id);
            assert_eq!(entity.kind, back.kind);
            assert_eq!(entity.name, back.name);
            assert_eq!(entity.exported, back.exported);
        });
}

// ── Edge JSON roundtrip ──────────────────────────────────────────────

#[test]
fn edge_json_roundtrip() {
    check!()
        .with_generator((
            bolero::generator::produce::<String>(),
            0..ALL_EDGE_KINDS.len(),
            bolero::generator::produce::<String>(),
        ))
        .for_each(|(src, rel_idx, dst)| {
            let rel = ALL_EDGE_KINDS[*rel_idx];
            let edge = Edge::new(src.clone(), rel, dst.clone());
            let json = serde_json::to_string(&edge).unwrap();
            let back: Edge = serde_json::from_str(&json).unwrap();
            assert_eq!(edge, back);
        });
}

// ── Store CRUD roundtrip properties ──────────────────────────────────

fn open_temp_store() -> (tempfile::TempDir, chizu_core::ChizuStore) {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::default();
    let store = chizu_core::ChizuStore::open(dir.path(), &config).unwrap();
    (dir, store)
}

#[test]
fn store_entity_insert_get_roundtrip() {
    check!()
        .with_generator((0..ALL_ENTITY_KINDS.len(), bolero::generator::produce::<bool>()))
        .for_each(|(kind_idx, exported)| {
            let kind = ALL_ENTITY_KINDS[*kind_idx];
            let (_dir, store) = open_temp_store();
            let entity =
                Entity::new("test::prop::1", kind, "prop_entity").with_exported(*exported);
            store.insert_entity(&entity).unwrap();
            let retrieved = store.get_entity("test::prop::1").unwrap().unwrap();
            assert_eq!(entity.kind, retrieved.kind);
            assert_eq!(entity.exported, retrieved.exported);
            assert_eq!(entity.name, retrieved.name);
        });
}

#[test]
fn store_edge_insert_get_roundtrip() {
    check!()
        .with_generator(0..ALL_EDGE_KINDS.len())
        .for_each(|idx: &usize| {
            let rel = ALL_EDGE_KINDS[*idx];
            let (_dir, store) = open_temp_store();
            let edge = Edge::new("src::a", rel, "dst::b");
            store.insert_edge(&edge).unwrap();
            let edges = store.get_edges_from("src::a").unwrap();
            assert_eq!(edges.len(), 1);
            assert_eq!(edges[0].rel, rel);
            assert_eq!(edges[0].dst_id, "dst::b");
        });
}

#[test]
fn store_file_insert_get_roundtrip() {
    check!()
        .with_generator(0..ALL_FILE_KINDS.len())
        .for_each(|idx: &usize| {
            let kind = ALL_FILE_KINDS[*idx];
            let (_dir, store) = open_temp_store();
            let record = FileRecord::new("test/file.rs", kind, "deadbeef");
            store.insert_file(&record).unwrap();
            let retrieved = store.get_file("test/file.rs").unwrap().unwrap();
            assert_eq!(retrieved.kind, kind);
            assert_eq!(retrieved.hash, "deadbeef");
        });
}

#[test]
fn store_entity_insert_is_idempotent() {
    check!()
        .with_generator(0..ALL_ENTITY_KINDS.len())
        .for_each(|idx: &usize| {
            let kind = ALL_ENTITY_KINDS[*idx];
            let (_dir, store) = open_temp_store();
            let entity = Entity::new("idem::1", kind, "first");
            store.insert_entity(&entity).unwrap();
            let entity2 = Entity::new("idem::1", kind, "second");
            store.insert_entity(&entity2).unwrap();
            let retrieved = store.get_entity("idem::1").unwrap().unwrap();
            assert_eq!(retrieved.name, "second");
        });
}

#[test]
fn store_delete_nonexistent_never_errors() {
    check!().with_type::<String>().for_each(|id: &String| {
        let (_dir, store) = open_temp_store();
        assert!(store.delete_entity(id).is_ok());
        assert!(store.delete_file(id).is_ok());
        assert!(store.delete_summary(id).is_ok());
    });
}

// ── Fuzzing: parsers must never panic on arbitrary input ─────────────

#[test]
fn fuzz_entity_kind_from_str_no_panic() {
    check!().with_type::<String>().for_each(|s: &String| {
        let _ = s.parse::<EntityKind>();
    });
}

#[test]
fn fuzz_edge_kind_from_str_no_panic() {
    check!().with_type::<String>().for_each(|s: &String| {
        let _ = s.parse::<EdgeKind>();
    });
}

#[test]
fn fuzz_visibility_from_str_no_panic() {
    check!().with_type::<String>().for_each(|s: &String| {
        let _ = s.parse::<Visibility>();
    });
}

#[test]
fn fuzz_file_kind_from_str_no_panic() {
    check!().with_type::<String>().for_each(|s: &String| {
        let _ = s.parse::<FileKind>();
    });
}

#[test]
fn fuzz_component_id_parse_no_panic() {
    check!().with_type::<String>().for_each(|s: &String| {
        let _ = ComponentId::parse(s);
    });
}

#[test]
fn fuzz_config_from_toml_no_panic() {
    check!().with_type::<String>().for_each(|s: &String| {
        let _ = Config::from_toml(s);
    });
}

#[test]
fn fuzz_entity_json_deser_no_panic() {
    check!()
        .with_type::<Vec<u8>>()
        .for_each(|bytes: &Vec<u8>| {
            let _ = serde_json::from_slice::<Entity>(bytes);
        });
}

#[test]
fn fuzz_edge_json_deser_no_panic() {
    check!()
        .with_type::<Vec<u8>>()
        .for_each(|bytes: &Vec<u8>| {
            let _ = serde_json::from_slice::<Edge>(bytes);
        });
}

// ── Config TOML roundtrip ────────────────────────────────────────────

#[test]
fn config_default_roundtrip_through_toml() {
    let original = Config::default();
    let toml_str = original.to_toml().unwrap();
    let parsed = Config::from_toml(&toml_str).unwrap();
    assert_eq!(original.search.default_limit, parsed.search.default_limit);
    assert_eq!(
        original.search.rerank_weights.keyword,
        parsed.search.rerank_weights.keyword
    );
    assert_eq!(original.embedding.dimensions, parsed.embedding.dimensions);
}

// ── Hash determinism ─────────────────────────────────────────────────

#[test]
fn blake3_hash_is_deterministic() {
    check!()
        .with_type::<Vec<u8>>()
        .for_each(|data: &Vec<u8>| {
            let h1 = blake3::hash(data);
            let h2 = blake3::hash(data);
            assert_eq!(h1, h2);
        });
}

#[test]
fn usearch_key_is_deterministic() {
    check!().with_type::<String>().for_each(|id: &String| {
        let k1 = chizu_core::entity_id_to_usearch_key(id);
        let k2 = chizu_core::entity_id_to_usearch_key(id);
        assert_eq!(k1, k2);
    });
}

// ── Summary roundtrip through store ──────────────────────────────────

#[test]
fn store_summary_roundtrip() {
    check!()
        .with_type::<(String, String)>()
        .for_each(|(short, detailed)| {
            if short.is_empty() {
                return;
            }
            let (_dir, store) = open_temp_store();
            let entity = Entity::new("summary::target", EntityKind::Symbol, "sym");
            store.insert_entity(&entity).unwrap();

            let summary =
                Summary::new("summary::target", short.clone()).with_detailed(detailed.clone());
            store.insert_summary(&summary).unwrap();
            let retrieved = store.get_summary("summary::target").unwrap().unwrap();
            assert_eq!(retrieved.short_summary, *short);
            assert_eq!(retrieved.detailed_summary, Some(detailed.clone()));
        });
}

// ── TaskRoute priority roundtrip ─────────────────────────────────────

#[test]
fn store_task_route_roundtrip() {
    check!()
        .with_generator(-1000..=1000i32)
        .for_each(|priority: &i32| {
            let (_dir, store) = open_temp_store();
            let route = TaskRoute::new("test_task", "entity::1", *priority);
            store.insert_task_route(&route).unwrap();
            let routes = store.get_task_routes("test_task").unwrap();
            assert_eq!(routes.len(), 1);
            assert_eq!(routes[0].priority, *priority);
        });
}
