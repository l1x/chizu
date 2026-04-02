use chizu_core::store::sqlite::SqliteStore;
use chizu_core::{ChizuStore, Store, StoreError};

/// Remove the usearch vector and embedding metadata for an entity.
fn remove_entity_vector(
    store: &ChizuStore,
    sqlite: &SqliteStore,
    entity_id: &str,
) -> Result<(), StoreError> {
    if let Some(meta) = sqlite.get_embedding_meta(entity_id)? {
        if let Some(key) = meta.usearch_key {
            store.remove_vector(key)?;
        }
    }
    sqlite.delete_embedding_meta(entity_id)?;
    Ok(())
}

/// Cascade-delete a single entity and all its derived data:
/// summary, embedding metadata, usearch vector, task routes, and edges from/to it.
pub fn cascade_delete_entity(store: &ChizuStore, entity_id: &str) -> Result<(), StoreError> {
    let sqlite = store.sqlite();

    remove_entity_vector(store, sqlite, entity_id)?;
    sqlite.delete_summary(entity_id)?;
    sqlite.delete_entity_task_routes(entity_id)?;
    for edge in sqlite.get_edges_from(entity_id)? {
        sqlite.delete_edge(&edge.src_id, edge.rel, &edge.dst_id)?;
    }
    for edge in sqlite.get_edges_to(entity_id)? {
        sqlite.delete_edge(&edge.src_id, edge.rel, &edge.dst_id)?;
    }
    sqlite.delete_entity(entity_id)?;
    Ok(())
}

/// Cascade-delete all data associated with a file path.
///
/// Deletes: entities (and their summaries/embeddings/vectors/task routes),
/// edges provenanced to this file or referencing deleted entities,
/// and the file record itself.
pub fn cascade_delete_file(store: &ChizuStore, path: &str) -> Result<(), StoreError> {
    let sqlite = store.sqlite();
    let entities = sqlite.get_entities_by_path(path)?;

    for entity in &entities {
        remove_entity_vector(store, sqlite, &entity.id)?;
        sqlite.delete_summary(&entity.id)?;
        sqlite.delete_entity_task_routes(&entity.id)?;
    }

    sqlite.delete_edges_by_provenance_path(path)?;

    // Also delete edges referencing these entities that were created by
    // workspace-level adapters (which have no file provenance).
    for entity in &entities {
        for edge in sqlite.get_edges_from(&entity.id)? {
            sqlite.delete_edge(&edge.src_id, edge.rel, &edge.dst_id)?;
        }
        for edge in sqlite.get_edges_to(&entity.id)? {
            sqlite.delete_edge(&edge.src_id, edge.rel, &edge.dst_id)?;
        }
    }

    sqlite.delete_entities_by_path(path)?;
    sqlite.delete_file(path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{
        ComponentId, Config, Edge, EdgeKind, EmbeddingMeta, Entity, EntityKind, FileKind,
        FileRecord, Summary, TaskRoute, entity_id_to_usearch_key,
    };
    use tempfile::TempDir;

    fn create_test_store() -> (ChizuStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let store = ChizuStore::open(temp_dir.path(), &config).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn cascade_delete_removes_all_file_data() {
        let (store, _temp) = create_test_store();
        let sqlite = store.sqlite();

        sqlite
            .insert_file(&FileRecord::new("src/lib.rs", FileKind::Source, "abc"))
            .unwrap();

        let symbol = Entity::new("symbol::src/lib.rs::foo", EntityKind::Symbol, "foo")
            .with_path("src/lib.rs")
            .with_component(ComponentId::new("cargo", "."));
        let test = Entity::new("test::src/lib.rs::test_foo", EntityKind::Test, "test_foo")
            .with_path("src/lib.rs");
        sqlite.insert_entity(&symbol).unwrap();
        sqlite.insert_entity(&test).unwrap();

        sqlite
            .insert_edge(&Edge::new(
                "source_unit::src/lib.rs",
                EdgeKind::Defines,
                "symbol::src/lib.rs::foo",
            ))
            .unwrap();
        sqlite
            .insert_edge(
                &Edge::new(
                    "source_unit::src/lib.rs",
                    EdgeKind::TestedBy,
                    "test::src/lib.rs::test_foo",
                )
                .with_provenance("src/lib.rs", 10),
            )
            .unwrap();

        sqlite
            .insert_summary(&Summary::new("symbol::src/lib.rs::foo", "short"))
            .unwrap();
        sqlite
            .insert_task_route(&TaskRoute::new("debug", "symbol::src/lib.rs::foo", 80))
            .unwrap();

        cascade_delete_file(&store, "src/lib.rs").unwrap();

        assert!(sqlite.get_file("src/lib.rs").unwrap().is_none());
        assert!(
            sqlite
                .get_entity("symbol::src/lib.rs::foo")
                .unwrap()
                .is_none()
        );
        assert!(
            sqlite
                .get_entity("test::src/lib.rs::test_foo")
                .unwrap()
                .is_none()
        );
        assert!(
            sqlite
                .get_summary("symbol::src/lib.rs::foo")
                .unwrap()
                .is_none()
        );
        assert!(
            sqlite
                .get_entity_task_routes("symbol::src/lib.rs::foo")
                .unwrap()
                .is_empty()
        );
        assert!(
            sqlite
                .get_edges_from("source_unit::src/lib.rs")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cascade_delete_removes_usearch_vectors() {
        let (store, _temp) = create_test_store();
        let sqlite = store.sqlite();

        let entity_id = "symbol::src/lib.rs::foo";
        sqlite
            .insert_entity(
                &Entity::new(entity_id, EntityKind::Symbol, "foo").with_path("src/lib.rs"),
            )
            .unwrap();

        // Insert a vector + metadata
        let key = entity_id_to_usearch_key(entity_id);
        let vector = vec![1.0_f32; store.vector_dimensions()];
        store.add_vector(entity_id, key, &vector).unwrap();
        let meta = EmbeddingMeta::new(entity_id, "test-model", store.vector_dimensions() as u32)
            .with_usearch_key(key);
        sqlite.insert_embedding_meta(&meta).unwrap();

        assert!(store.contains_vector(key));

        cascade_delete_entity(&store, entity_id).unwrap();

        assert!(!store.contains_vector(key));
        assert!(sqlite.get_embedding_meta(entity_id).unwrap().is_none());
        assert!(sqlite.get_entity(entity_id).unwrap().is_none());
    }

    #[test]
    fn cascade_delete_nonexistent_succeeds() {
        let (store, _temp) = create_test_store();
        cascade_delete_file(&store, "no/such/file.rs").unwrap();
    }
}
