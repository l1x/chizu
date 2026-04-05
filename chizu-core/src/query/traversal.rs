use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{EdgeKind, Entity};
use crate::store::{Result, Store};

/// Result of a BFS graph traversal.
pub struct TraversalResult {
    /// Entities selected by the traversal.
    pub entities: HashMap<String, Entity>,
    /// Edges between selected entities (src_id, rel, dst_id).
    pub edges: HashSet<(String, EdgeKind, String)>,
}

/// Options controlling a BFS graph traversal.
pub struct TraversalOptions<'a> {
    /// Maximum BFS depth from the seed nodes.
    pub max_depth: u32,
    /// Maximum number of entities to collect.
    pub max_nodes: usize,
    /// If set, only include entities whose kind (as string) is in this list.
    pub kind_filter: Option<&'a [String]>,
    /// Exclude entities whose ID contains any of these patterns.
    pub exclude_patterns: &'a [String],
}

/// Perform a BFS traversal over the graph in `store`, starting from `seed_ids`.
///
/// Queries entities and edges on demand (bounded by `max_nodes`), so cost
/// scales with the reachable subgraph rather than total repo size.
/// Returns the set of visited entities and all edges between them.
pub fn graph_traversal(
    store: &dyn Store,
    seed_ids: &[String],
    opts: &TraversalOptions,
) -> Result<TraversalResult> {
    let mut entity_cache: HashMap<String, Entity> = HashMap::new();
    let mut visited_edges: HashSet<(String, EdgeKind, String)> = HashSet::new();
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();

    for id in seed_ids {
        queue.push_back((id.clone(), 0));
    }

    while let Some((entity_id, depth)) = queue.pop_front() {
        if entity_cache.contains_key(&entity_id) {
            continue;
        }
        if entity_cache.len() >= opts.max_nodes {
            break;
        }

        let Some(entity) = store.get_entity(&entity_id)? else {
            continue;
        };

        if let Some(kinds) = opts.kind_filter {
            if !kinds.contains(&entity.kind.to_string()) {
                continue;
            }
        }
        if opts
            .exclude_patterns
            .iter()
            .any(|p| entity.id.contains(p))
        {
            continue;
        }

        entity_cache.insert(entity_id.clone(), entity);

        if depth < opts.max_depth {
            for edge in store.get_edges_from(&entity_id)? {
                let key = (edge.src_id.clone(), edge.rel, edge.dst_id.clone());
                if visited_edges.insert(key) {
                    queue.push_back((edge.dst_id.clone(), depth + 1));
                }
            }
            for edge in store.get_edges_to(&entity_id)? {
                let key = (edge.src_id.clone(), edge.rel, edge.dst_id.clone());
                if visited_edges.insert(key) {
                    queue.push_back((edge.src_id.clone(), depth + 1));
                }
            }
        }
    }

    // Collect edges between selected entities.
    let selected_ids: HashSet<&str> = entity_cache.keys().map(|s| s.as_str()).collect();
    let mut render_edges: HashSet<(String, EdgeKind, String)> = HashSet::new();
    for entity_id in &selected_ids {
        for edge in store.get_edges_from(entity_id)? {
            if selected_ids.contains(edge.dst_id.as_str()) {
                render_edges.insert((edge.src_id.clone(), edge.rel, edge.dst_id.clone()));
            }
        }
    }

    Ok(TraversalResult {
        entities: entity_cache,
        edges: render_edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind, Entity, EntityKind};
    use crate::store::ChizuStore;

    #[test]
    fn traversal_respects_depth_limit() {
        let (store, _tmp) = ChizuStore::open_test(None);
        store
            .insert_entity(&Entity::new("a", EntityKind::Repo, "a"))
            .unwrap();
        store
            .insert_entity(&Entity::new("b", EntityKind::Component, "b"))
            .unwrap();
        store
            .insert_entity(&Entity::new("c", EntityKind::Symbol, "c"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Contains, "b"))
            .unwrap();
        store
            .insert_edge(&Edge::new("b", EdgeKind::Defines, "c"))
            .unwrap();

        let result = graph_traversal(
            &store,
            &["a".into()],
            &TraversalOptions {
                max_depth: 1,
                max_nodes: 100,
                kind_filter: None,
                exclude_patterns: &[],
            },
        )
        .unwrap();

        assert!(result.entities.contains_key("a"));
        assert!(result.entities.contains_key("b"));
        assert!(
            !result.entities.contains_key("c"),
            "depth 2 should not be reached"
        );
    }

    #[test]
    fn traversal_respects_max_nodes() {
        let (store, _tmp) = ChizuStore::open_test(None);
        store
            .insert_entity(&Entity::new("a", EntityKind::Repo, "a"))
            .unwrap();
        store
            .insert_entity(&Entity::new("b", EntityKind::Component, "b"))
            .unwrap();
        store
            .insert_entity(&Entity::new("c", EntityKind::Component, "c"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Contains, "b"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Contains, "c"))
            .unwrap();

        let result = graph_traversal(
            &store,
            &["a".into()],
            &TraversalOptions {
                max_depth: 10,
                max_nodes: 2,
                kind_filter: None,
                exclude_patterns: &[],
            },
        )
        .unwrap();

        assert_eq!(result.entities.len(), 2);
    }

    #[test]
    fn traversal_filters_by_kind() {
        let (store, _tmp) = ChizuStore::open_test(None);
        store
            .insert_entity(&Entity::new("a", EntityKind::Repo, "a"))
            .unwrap();
        store
            .insert_entity(&Entity::new("b", EntityKind::Component, "b"))
            .unwrap();
        store
            .insert_entity(&Entity::new("c", EntityKind::Symbol, "c"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Contains, "b"))
            .unwrap();
        store
            .insert_edge(&Edge::new("b", EdgeKind::Defines, "c"))
            .unwrap();
        let kinds = vec!["repo".to_string(), "component".to_string()];

        let result = graph_traversal(
            &store,
            &["a".into()],
            &TraversalOptions {
                max_depth: 10,
                max_nodes: 100,
                kind_filter: Some(&kinds),
                exclude_patterns: &[],
            },
        )
        .unwrap();

        assert!(result.entities.contains_key("a"));
        assert!(result.entities.contains_key("b"));
        assert!(!result.entities.contains_key("c"));
    }

    #[test]
    fn traversal_excludes_patterns() {
        let (store, _tmp) = ChizuStore::open_test(None);
        store
            .insert_entity(&Entity::new("a", EntityKind::Repo, "a"))
            .unwrap();
        store
            .insert_entity(&Entity::new(
                "b::skip",
                EntityKind::Component,
                "b",
            ))
            .unwrap();
        store
            .insert_entity(&Entity::new("c", EntityKind::Component, "c"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Contains, "b::skip"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Contains, "c"))
            .unwrap();

        let result = graph_traversal(
            &store,
            &["a".into()],
            &TraversalOptions {
                max_depth: 10,
                max_nodes: 100,
                kind_filter: None,
                exclude_patterns: &["skip".to_string()],
            },
        )
        .unwrap();

        assert!(result.entities.contains_key("a"));
        assert!(!result.entities.contains_key("b::skip"));
        assert!(result.entities.contains_key("c"));
    }

    #[test]
    fn traversal_collects_edges_between_selected() {
        let (store, _tmp) = ChizuStore::open_test(None);
        store
            .insert_entity(&Entity::new("a", EntityKind::Repo, "a"))
            .unwrap();
        store
            .insert_entity(&Entity::new("b", EntityKind::Component, "b"))
            .unwrap();
        store
            .insert_entity(&Entity::new("c", EntityKind::Symbol, "c"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Contains, "b"))
            .unwrap();
        store
            .insert_edge(&Edge::new("b", EdgeKind::Defines, "c"))
            .unwrap();

        let result = graph_traversal(
            &store,
            &["a".into()],
            &TraversalOptions {
                max_depth: 10,
                max_nodes: 100,
                kind_filter: None,
                exclude_patterns: &[],
            },
        )
        .unwrap();

        assert_eq!(result.entities.len(), 3);
        assert!(result
            .edges
            .contains(&("a".into(), EdgeKind::Contains, "b".into())));
        assert!(result
            .edges
            .contains(&("b".into(), EdgeKind::Defines, "c".into())));
    }
}
