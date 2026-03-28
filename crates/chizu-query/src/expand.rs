use std::collections::{HashMap, HashSet};

use chizu_core::model::EdgeKind;
use chizu_core::Store;

use crate::retrieve::{Candidate, RetrievalSource};

const USEFUL_EDGE_KINDS: &[EdgeKind] = &[
    EdgeKind::Contains,
    EdgeKind::Defines,
    EdgeKind::DependsOn,
    EdgeKind::Implements,
    EdgeKind::TestedBy,
    EdgeKind::BenchmarkedBy,
    EdgeKind::DocumentedBy,
    EdgeKind::Reexports,
    EdgeKind::ConfiguredBy,
    EdgeKind::RelatedTo,
    EdgeKind::Migrates,
    EdgeKind::Specifies,
    EdgeKind::Renders,
    EdgeKind::Deploys,
    EdgeKind::Builds,
];

/// Expand candidates by 1-hop graph traversal.
///
/// Returns the neighbor candidates keyed by entity_id, with `context_via`
/// set to the seed entity they were reached from.
pub fn expand(
    store: &Store,
    seeds: &HashMap<String, Candidate>,
    max_neighbors_per_seed: usize,
) -> chizu_core::Result<HashMap<String, (Candidate, String)>> {
    let seed_ids: HashSet<&str> = seeds.keys().map(String::as_str).collect();
    let mut neighbors: HashMap<String, (Candidate, String)> = HashMap::new();

    for seed_id in seeds.keys() {
        let mut count = 0;

        // Outgoing edges
        let outgoing = store.edges_from(seed_id)?;
        for edge in &outgoing {
            if count >= max_neighbors_per_seed {
                break;
            }
            if !USEFUL_EDGE_KINDS.contains(&edge.rel) {
                continue;
            }
            if seed_ids.contains(edge.dst_id.as_str()) || neighbors.contains_key(&edge.dst_id) {
                continue;
            }
            if let Ok(entity) = store.get_entity(&edge.dst_id) {
                let (short_summary, keywords) = load_summary(store, &entity.id);
                let candidate = Candidate {
                    entity,
                    sources: vec![RetrievalSource::Context { via_entity_id: seed_id.clone() }],
                    short_summary,
                    keywords,
                };
                neighbors.insert(candidate.entity.id.clone(), (candidate, seed_id.clone()));
                count += 1;
            }
        }

        // Incoming edges
        let incoming = store.edges_to(seed_id)?;
        for edge in &incoming {
            if count >= max_neighbors_per_seed {
                break;
            }
            if !USEFUL_EDGE_KINDS.contains(&edge.rel) {
                continue;
            }
            if seed_ids.contains(edge.src_id.as_str()) || neighbors.contains_key(&edge.src_id) {
                continue;
            }
            if let Ok(entity) = store.get_entity(&edge.src_id) {
                let (short_summary, keywords) = load_summary(store, &entity.id);
                let candidate = Candidate {
                    entity,
                    sources: vec![RetrievalSource::Context { via_entity_id: seed_id.clone() }],
                    short_summary,
                    keywords,
                };
                neighbors.insert(candidate.entity.id.clone(), (candidate, seed_id.clone()));
                count += 1;
            }
        }
    }

    Ok(neighbors)
}

fn load_summary(store: &Store, entity_id: &str) -> (String, Vec<String>) {
    match store.get_summary(entity_id) {
        Ok(s) => (s.short_summary, s.keywords),
        Err(_) => (String::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::model::{Edge, Entity, EntityKind, Summary};
    use chizu_core::Store;

    fn make_entity(id: &str, name: &str, kind: EntityKind) -> Entity {
        Entity {
            id: id.to_string(),
            kind,
            name: name.to_string(),
            component_id: None,
            path: Some(format!("src/{name}.rs")),
            language: Some("rust".to_string()),
            line_start: Some(1),
            line_end: Some(50),
            visibility: Some("pub".to_string()),
            exported: true,
        }
    }

    fn make_candidate(entity: Entity) -> Candidate {
        Candidate {
            entity,
            sources: vec![RetrievalSource::NameMatch],
            short_summary: String::new(),
            keywords: Vec::new(),
        }
    }

    /// Build a graph: A --Contains--> B --DependsOn--> C
    ///                A <--TestedBy-- D
    ///                A --OwnsTask--> E  (OwnsTask is NOT in USEFUL_EDGE_KINDS)
    ///                A --Contains--> F, G, H, I, J, K  (6 extra for cap testing)
    fn build_expand_store() -> (Store, HashMap<String, Candidate>) {
        let store = Store::open_in_memory().unwrap();

        let a = make_entity("e::A", "A", EntityKind::Component);
        let b = make_entity("e::B", "B", EntityKind::Symbol);
        let c = make_entity("e::C", "C", EntityKind::Symbol);
        let d = make_entity("e::D", "D", EntityKind::Test);
        let e = make_entity("e::E", "E", EntityKind::Containerized);

        for ent in [&a, &b, &c, &d, &e] {
            store.insert_entity(ent).unwrap();
        }

        // Extra entities for cap testing
        for suffix in ["F", "G", "H", "I", "J", "K"] {
            let ent = make_entity(&format!("e::{suffix}"), suffix, EntityKind::SourceUnit);
            store.insert_entity(&ent).unwrap();
            store
                .insert_edge(&Edge {
                    src_id: "e::A".to_string(),
                    rel: EdgeKind::Contains,
                    dst_id: format!("e::{suffix}"),
                    provenance_path: None,
                    provenance_line: None,
                })
                .unwrap();
        }

        // A --Contains--> B
        store
            .insert_edge(&Edge {
                src_id: "e::A".to_string(),
                rel: EdgeKind::Contains,
                dst_id: "e::B".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();

        // B --DependsOn--> C
        store
            .insert_edge(&Edge {
                src_id: "e::B".to_string(),
                rel: EdgeKind::DependsOn,
                dst_id: "e::C".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();

        // D --TestedBy (incoming to A, i.e. A is tested by D)
        // Actually edges_to("e::A") means edges where dst_id = "e::A"
        // So: D --TestedBy--> A means edges_to("e::A") returns this edge
        store
            .insert_edge(&Edge {
                src_id: "e::D".to_string(),
                rel: EdgeKind::TestedBy,
                dst_id: "e::A".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();

        // A --OwnsTask--> E (OwnsTask is not in USEFUL_EDGE_KINDS)
        store
            .insert_edge(&Edge {
                src_id: "e::A".to_string(),
                rel: EdgeKind::OwnsTask,
                dst_id: "e::E".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();

        // Summary on B
        store
            .upsert_summary(&Summary {
                entity_id: "e::B".to_string(),
                short_summary: "Symbol B".to_string(),
                detailed_summary: None,
                keywords: vec!["bravo".to_string()],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                source_hash: None,
            })
            .unwrap();

        // Seeds: only A
        let mut seeds = HashMap::new();
        seeds.insert("e::A".to_string(), make_candidate(a));

        (store, seeds)
    }

    #[test]
    fn expand_finds_outgoing_useful_edges() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        // A --Contains--> B (useful), so B should be a neighbor
        assert!(
            neighbors.contains_key("e::B"),
            "B should be found via outgoing Contains"
        );
        let (_, via) = &neighbors["e::B"];
        assert_eq!(via, "e::A");
    }

    #[test]
    fn expand_finds_incoming_useful_edges() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        // D --TestedBy--> A, so edges_to("e::A") finds D
        assert!(
            neighbors.contains_key("e::D"),
            "D should be found via incoming TestedBy"
        );
    }

    #[test]
    fn expand_skips_non_useful_edge_kinds() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        // A --OwnsTask--> E, but OwnsTask is not in USEFUL_EDGE_KINDS
        assert!(
            !neighbors.contains_key("e::E"),
            "E should be skipped (OwnsTask not useful)"
        );
    }

    #[test]
    fn expand_skips_seeds() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        // A is a seed, should not appear as a neighbor
        assert!(!neighbors.contains_key("e::A"));
    }

    #[test]
    fn expand_does_not_follow_two_hops() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        // C is reachable via A->B->C, but expand only does 1-hop from seeds
        // C is not directly connected to A
        assert!(
            !neighbors.contains_key("e::C"),
            "C should not be found (2 hops away)"
        );
    }

    #[test]
    fn expand_respects_max_neighbors_cap() {
        let (store, seeds) = build_expand_store();
        // A has 7 outgoing Contains edges (B, F, G, H, I, J, K) + 1 Builds (skipped)
        // Plus 1 incoming TestedBy (D)
        // With cap of 3, should only get 3 neighbors
        let neighbors = expand(&store, &seeds, 3).unwrap();
        assert_eq!(neighbors.len(), 3, "should be capped at 3 neighbors");
    }

    #[test]
    fn expand_cap_one() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 1).unwrap();
        assert_eq!(neighbors.len(), 1);
    }

    #[test]
    fn expand_cap_zero() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 0).unwrap();
        assert!(neighbors.is_empty());
    }

    #[test]
    fn expand_deduplicates_across_seeds() {
        let store = Store::open_in_memory().unwrap();

        let a = make_entity("e::A", "A", EntityKind::Component);
        let b = make_entity("e::B", "B", EntityKind::Component);
        let shared = make_entity("e::Shared", "Shared", EntityKind::Symbol);

        for ent in [&a, &b, &shared] {
            store.insert_entity(ent).unwrap();
        }

        // Both A and B connect to Shared
        store
            .insert_edge(&Edge {
                src_id: "e::A".to_string(),
                rel: EdgeKind::Contains,
                dst_id: "e::Shared".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();
        store
            .insert_edge(&Edge {
                src_id: "e::B".to_string(),
                rel: EdgeKind::Contains,
                dst_id: "e::Shared".to_string(),
                provenance_path: None,
                provenance_line: None,
            })
            .unwrap();

        let mut seeds = HashMap::new();
        seeds.insert("e::A".to_string(), make_candidate(a));
        seeds.insert("e::B".to_string(), make_candidate(b));

        let neighbors = expand(&store, &seeds, 10).unwrap();

        // Shared should appear only once
        assert_eq!(
            neighbors
                .values()
                .filter(|(c, _)| c.entity.id == "e::Shared")
                .count(),
            1
        );
    }

    #[test]
    fn expand_loads_summary_for_neighbors() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        let (b_cand, _) = &neighbors["e::B"];
        assert_eq!(b_cand.short_summary, "Symbol B");
        assert_eq!(b_cand.keywords, vec!["bravo".to_string()]);
    }

    #[test]
    fn expand_neighbor_without_summary_gets_empty() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        // D has no summary
        let (d_cand, _) = &neighbors["e::D"];
        assert_eq!(d_cand.short_summary, "");
        assert!(d_cand.keywords.is_empty());
    }

    #[test]
    fn expand_empty_seeds() {
        let store = Store::open_in_memory().unwrap();
        let seeds: HashMap<String, Candidate> = HashMap::new();
        let neighbors = expand(&store, &seeds, 10).unwrap();
        assert!(neighbors.is_empty());
    }

    #[test]
    fn expand_context_via_is_seed_id() {
        let (store, seeds) = build_expand_store();
        let neighbors = expand(&store, &seeds, 10).unwrap();

        for (_, (_, via)) in &neighbors {
            assert_eq!(via, "e::A", "all neighbors should trace back to seed A");
        }
    }
}
