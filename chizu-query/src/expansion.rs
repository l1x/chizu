use std::collections::HashSet;

use chizu_core::Store;

use crate::error::Result;
use crate::retrieval::Candidate;

/// Expand seed candidates with 1-hop graph neighbors.
///
/// - Takes the top `limit` seeds by current `final_score`.
/// - Fetches up to 5 unique neighbors per seed.
/// - Adds neighbors as new candidates with `is_context: true`.
/// - Does not expand from context nodes.
pub fn expand(store: &dyn Store, candidates: &mut Vec<Candidate>, limit: usize) -> Result<()> {
    let seed_ids: Vec<String> = candidates
        .iter()
        .filter(|c| !c.is_context)
        .take(limit)
        .map(|c| c.entity.id.clone())
        .collect();

    let mut existing_ids: HashSet<String> =
        candidates.iter().map(|c| c.entity.id.clone()).collect();

    for seed_id in seed_ids {
        let mut neighbors = Vec::new();

        // Outgoing edges
        for edge in store.get_edges_from(&seed_id)? {
            neighbors.push(edge.dst_id);
        }

        // Incoming edges
        for edge in store.get_edges_to(&seed_id)? {
            neighbors.push(edge.src_id);
        }

        let mut added = 0;
        for neighbor_id in neighbors {
            if existing_ids.contains(&neighbor_id) {
                continue;
            }
            if let Some(entity) = store.get_entity(&neighbor_id)? {
                let mut c = Candidate::from_entity(entity);
                c.is_context = true;
                candidates.push(c);
                existing_ids.insert(neighbor_id);
                added += 1;
                if added >= 5 {
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::Candidate;
    use chizu_core::{ChizuStore, Config, Edge, EdgeKind, Entity, EntityKind, Store};
    use tempfile::TempDir;

    fn create_test_store() -> (ChizuStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let store = ChizuStore::open(temp_dir.path(), &config).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_expansion_adds_neighbors() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("a", EntityKind::Symbol, "a"))
            .unwrap();
        store
            .insert_entity(&Entity::new("b", EntityKind::Symbol, "b"))
            .unwrap();
        store
            .insert_entity(&Entity::new("c", EntityKind::Symbol, "c"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Defines, "b"))
            .unwrap();
        store
            .insert_edge(&Edge::new("c", EdgeKind::Defines, "a"))
            .unwrap();

        let mut candidates = vec![Candidate {
            entity: Entity::new("a", EntityKind::Symbol, "a"),
            task_route_priority: None,
            keyword_score: 1.0,
            name_match_score: 0.0,
            path_match_score: 0.0,
            vector_score: 0.0,
            is_context: false,
            final_score: 1.0,
        }];

        expand(&store, &mut candidates, 10).unwrap();

        assert_eq!(candidates.len(), 3);
        let context_ids: Vec<String> = candidates
            .iter()
            .filter(|c| c.is_context)
            .map(|c| c.entity.id.clone())
            .collect();
        assert!(context_ids.contains(&"b".to_string()));
        assert!(context_ids.contains(&"c".to_string()));
    }

    #[test]
    fn test_expansion_caps_at_5_per_seed() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("seed", EntityKind::Symbol, "seed"))
            .unwrap();
        for i in 0..10 {
            let id = format!("n{}", i);
            store
                .insert_entity(&Entity::new(&id, EntityKind::Symbol, &id))
                .unwrap();
            store
                .insert_edge(&Edge::new("seed", EdgeKind::Defines, &id))
                .unwrap();
        }

        let mut candidates = vec![Candidate {
            entity: Entity::new("seed", EntityKind::Symbol, "seed"),
            task_route_priority: None,
            keyword_score: 1.0,
            name_match_score: 0.0,
            path_match_score: 0.0,
            vector_score: 0.0,
            is_context: false,
            final_score: 1.0,
        }];

        expand(&store, &mut candidates, 10).unwrap();

        let context_count = candidates.iter().filter(|c| c.is_context).count();
        assert_eq!(context_count, 5);
    }

    #[test]
    fn test_expansion_does_not_expand_context() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("seed", EntityKind::Symbol, "seed"))
            .unwrap();
        store
            .insert_entity(&Entity::new("ctx", EntityKind::Symbol, "ctx"))
            .unwrap();
        store
            .insert_entity(&Entity::new("far", EntityKind::Symbol, "far"))
            .unwrap();
        store
            .insert_edge(&Edge::new("seed", EdgeKind::Defines, "ctx"))
            .unwrap();
        store
            .insert_edge(&Edge::new("ctx", EdgeKind::Defines, "far"))
            .unwrap();

        let mut candidates = vec![
            Candidate {
                entity: Entity::new("seed", EntityKind::Symbol, "seed"),
                task_route_priority: None,
                keyword_score: 1.0,
                name_match_score: 0.0,
                path_match_score: 0.0,
                vector_score: 0.0,
                is_context: false,
                final_score: 1.0,
            },
            Candidate {
                entity: Entity::new("ctx", EntityKind::Symbol, "ctx"),
                task_route_priority: None,
                keyword_score: 0.5,
                name_match_score: 0.0,
                path_match_score: 0.0,
                vector_score: 0.0,
                is_context: true,
                final_score: 0.5,
            },
        ];

        expand(&store, &mut candidates, 10).unwrap();

        let ids: Vec<String> = candidates.iter().map(|c| c.entity.id.clone()).collect();
        assert!(!ids.contains(&"far".to_string()));
    }
}
