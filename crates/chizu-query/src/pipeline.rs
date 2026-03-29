use chizu_core::Store;
use tracing::{debug, info, instrument};

use crate::classify::TaskCategory;
use crate::expand::expand;
use crate::plan::{ReadingPlan, ReadingPlanItem};
use crate::rerank::{rerank, RerankWeights};
use crate::retrieve::{retrieve, tokenize_query};

/// Configuration for the query pipeline.
#[derive(Debug)]
pub struct PipelineConfig {
    /// Maximum results in the reading plan.
    pub limit: usize,
    /// Number of vector search results to retrieve.
    pub vector_k: usize,
    /// Maximum neighbor entities to expand per seed.
    pub max_neighbors_per_seed: usize,
    /// Override the heuristic query classification.
    pub category_override: Option<TaskCategory>,
    /// Weights for reranking signals.
    pub weights: RerankWeights,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            limit: 15,
            vector_k: 20,
            max_neighbors_per_seed: 5,
            category_override: None,
            weights: RerankWeights::default(),
        }
    }
}

/// The full query pipeline: classify → retrieve → expand → rerank → reading plan.
pub struct QueryPipeline;

impl QueryPipeline {
    /// Run the query pipeline with tracing spans for each stage.
    #[instrument(skip(store, query_embedding), fields(query_len = query.len()))]
    pub fn run(
        store: &Store,
        query: &str,
        query_embedding: Option<&[f32]>,
        config: &PipelineConfig,
    ) -> chizu_core::Result<ReadingPlan> {
        // 1. Classify
        let category = match config.category_override {
            Some(cat) => {
                debug!(category = %cat, "using category override");
                cat
            }
            None => {
                let cat = Self::classify(query);
                debug!(category = %cat, "query classified");
                cat
            }
        };

        // 2. Tokenize
        let query_tokens = Self::tokenize(query);
        debug!(token_count = query_tokens.len(), "query tokenized");

        // 3. Retrieve
        let route_names = category.route_names();
        let candidates = Self::retrieve(
            store,
            route_names,
            &query_tokens,
            query_embedding,
            config.vector_k,
        )?;

        let candidates_considered = candidates.len();
        let used_vector_search = query_embedding.is_some();
        info!(
            candidates = candidates_considered,
            used_vector_search, "retrieval complete"
        );

        // 4. Expand
        let neighbors = Self::expand(store, &candidates, config.max_neighbors_per_seed)?;
        debug!(neighbor_count = neighbors.len(), "expansion complete");

        // 5. Rerank
        let seeds: Vec<_> = candidates.into_values().collect();
        let neighbor_list: Vec<_> = neighbors.into_values().collect();
        let scored = Self::rerank(seeds, neighbor_list, &category, &query_tokens, config.limit, &config.weights);
        debug!(result_count = scored.len(), "reranking complete");

        // 6. Build reading plan
        let items = scored
            .into_iter()
            .map(|entry| ReadingPlanItem {
                entity_id: entry.candidate.entity.id,
                name: entry.candidate.entity.name,
                kind: entry.candidate.entity.kind.as_str().to_string(),
                path: entry.candidate.entity.path,
                line_start: entry.candidate.entity.line_start,
                line_end: entry.candidate.entity.line_end,
                short_summary: entry.candidate.short_summary,
                score: entry.score,
                reasons: entry.reasons,
                is_context: entry.is_context,
                context_via: entry.context_via,
            })
            .collect();

        Ok(ReadingPlan {
            query: query.to_string(),
            category: category.as_str().to_string(),
            items,
            candidates_considered,
            used_vector_search,
        })
    }

    #[instrument(skip(query), fields(query_len = query.len()))]
    fn classify(query: &str) -> TaskCategory {
        TaskCategory::classify(query)
    }

    #[instrument(skip(query), fields(query_len = query.len()))]
    fn tokenize(query: &str) -> Vec<String> {
        tokenize_query(query)
    }

    #[instrument(
        skip(store, query_tokens, query_embedding),
        fields(route_count = route_names.len(), vector_k)
    )]
    fn retrieve(
        store: &Store,
        route_names: &[&str],
        query_tokens: &[String],
        query_embedding: Option<&[f32]>,
        vector_k: usize,
    ) -> chizu_core::Result<std::collections::HashMap<String, crate::retrieve::Candidate>> {
        retrieve(store, route_names, query_tokens, query_embedding, vector_k)
    }

    #[instrument(skip(store, candidates), fields(candidate_count = candidates.len()))]
    fn expand(
        store: &Store,
        candidates: &std::collections::HashMap<String, crate::retrieve::Candidate>,
        max_neighbors: usize,
    ) -> chizu_core::Result<std::collections::HashMap<String, (crate::retrieve::Candidate, String)>>
    {
        expand(store, candidates, max_neighbors)
    }

    #[instrument(
        skip(seeds, neighbors, category, query_tokens),
        fields(seed_count = seeds.len(), neighbor_count = neighbors.len(), limit)
    )]
    fn rerank(
        seeds: Vec<crate::retrieve::Candidate>,
        neighbors: Vec<(crate::retrieve::Candidate, String)>,
        category: &TaskCategory,
        query_tokens: &[String],
        limit: usize,
        weights: &RerankWeights,
    ) -> Vec<crate::rerank::ScoredEntry> {
        rerank(seeds, neighbors, category, query_tokens, limit, weights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::model::{Edge, EdgeKind, Entity, EntityKind, Summary, TaskRoute};

    fn make_entity(id: &str, name: &str, kind: EntityKind, path: &str, exported: bool) -> Entity {
        Entity {
            id: id.to_string(),
            kind,
            name: name.to_string(),
            component_id: None,
            path: Some(path.to_string()),
            language: Some("rust".to_string()),
            line_start: Some(1),
            line_end: Some(100),
            visibility: if exported {
                Some("pub".to_string())
            } else {
                None
            },
            exported,
        }
    }

    /// Build a realistic small graph for end-to-end pipeline testing.
    ///
    /// Entities:
    ///   - component::store (Component, exported, path src/store/mod.rs)
    ///   - symbol::SqliteStore (Symbol, exported, path src/store/sqlite.rs)
    ///   - symbol::Entity (Symbol, exported, path src/model/entity.rs)
    ///   - source_unit::engine (SourceUnit, exported, path src/engine.rs)
    ///   - test::store_tests (Test, not exported, path tests/store_test.rs)
    ///   - doc::README (Doc, exported, path README.md)
    ///
    /// Edges:
    ///   - component::store --Contains--> symbol::SqliteStore
    ///   - symbol::SqliteStore --DependsOn--> symbol::Entity
    ///   - test::store_tests --TestedBy--> component::store  (reversed: store is tested by store_tests)
    ///   - source_unit::engine --Builds--> doc::README  (Builds, not useful for expansion)
    ///
    /// Summaries on store, SqliteStore, Entity
    /// Task routes: "understand" -> component::store (pri 80), "debug" -> symbol::Entity (pri 90)
    fn build_pipeline_store() -> Store {
        let store = Store::open_in_memory().unwrap();

        let entities = vec![
            make_entity(
                "component::store",
                "store",
                EntityKind::Component,
                "src/store/mod.rs",
                true,
            ),
            make_entity(
                "symbol::SqliteStore",
                "SqliteStore",
                EntityKind::Symbol,
                "src/store/sqlite.rs",
                true,
            ),
            make_entity(
                "symbol::Entity",
                "Entity",
                EntityKind::Symbol,
                "src/model/entity.rs",
                true,
            ),
            make_entity(
                "source_unit::engine",
                "engine",
                EntityKind::SourceUnit,
                "src/engine.rs",
                true,
            ),
            make_entity(
                "test::store_tests",
                "store_tests",
                EntityKind::Test,
                "tests/store_test.rs",
                false,
            ),
            make_entity("doc::README", "README", EntityKind::Doc, "README.md", true),
        ];

        for e in &entities {
            store.insert_entity(e).unwrap();
        }

        let edges = vec![
            Edge {
                src_id: "component::store".to_string(),
                rel: EdgeKind::Contains,
                dst_id: "symbol::SqliteStore".to_string(),
                provenance_path: None,
                provenance_line: None,
            },
            Edge {
                src_id: "symbol::SqliteStore".to_string(),
                rel: EdgeKind::DependsOn,
                dst_id: "symbol::Entity".to_string(),
                provenance_path: None,
                provenance_line: None,
            },
            Edge {
                src_id: "test::store_tests".to_string(),
                rel: EdgeKind::TestedBy,
                dst_id: "component::store".to_string(),
                provenance_path: None,
                provenance_line: None,
            },
            Edge {
                src_id: "source_unit::engine".to_string(),
                rel: EdgeKind::Builds,
                dst_id: "doc::README".to_string(),
                provenance_path: None,
                provenance_line: None,
            },
        ];

        for e in &edges {
            store.insert_edge(e).unwrap();
        }

        store
            .upsert_summary(&Summary {
                entity_id: "component::store".to_string(),
                short_summary: "Persistence layer for the graph".to_string(),
                detailed_summary: None,
                keywords: vec![
                    "store".to_string(),
                    "persistence".to_string(),
                    "database".to_string(),
                ],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                source_hash: None,
            })
            .unwrap();

        store
            .upsert_summary(&Summary {
                entity_id: "symbol::SqliteStore".to_string(),
                short_summary: "SQLite-backed store implementation".to_string(),
                detailed_summary: None,
                keywords: vec![
                    "sqlite".to_string(),
                    "store".to_string(),
                    "backend".to_string(),
                ],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                source_hash: None,
            })
            .unwrap();

        store
            .upsert_summary(&Summary {
                entity_id: "symbol::Entity".to_string(),
                short_summary: "Core entity model".to_string(),
                detailed_summary: None,
                keywords: vec![
                    "entity".to_string(),
                    "model".to_string(),
                    "graph".to_string(),
                ],
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                source_hash: None,
            })
            .unwrap();

        store
            .insert_task_route(&TaskRoute {
                task_name: "understand".to_string(),
                entity_id: "component::store".to_string(),
                priority: 80,
            })
            .unwrap();

        store
            .insert_task_route(&TaskRoute {
                task_name: "debug".to_string(),
                entity_id: "symbol::Entity".to_string(),
                priority: 90,
            })
            .unwrap();

        store
    }

    #[test]
    fn pipeline_basic_keyword_query() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "how does the store work", None, &config).unwrap();

        assert_eq!(plan.query, "how does the store work");
        assert_eq!(plan.category, "understand");
        assert!(!plan.used_vector_search);
        assert!(!plan.items.is_empty());
    }

    #[test]
    fn pipeline_classifies_correctly() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();

        let plan = QueryPipeline::run(&store, "fix the entity bug", None, &config).unwrap();
        assert_eq!(plan.category, "debug");

        let plan = QueryPipeline::run(&store, "explain the architecture", None, &config).unwrap();
        assert_eq!(plan.category, "understand");

        let plan = QueryPipeline::run(&store, "add a new feature", None, &config).unwrap();
        assert_eq!(plan.category, "build");
    }

    #[test]
    fn pipeline_finds_store_entities_for_store_query() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "how does the store work", None, &config).unwrap();

        let ids: Vec<&str> = plan.items.iter().map(|i| i.entity_id.as_str()).collect();
        assert!(
            ids.contains(&"component::store"),
            "should find component::store for store query, got: {ids:?}"
        );
    }

    #[test]
    fn pipeline_expands_neighbors() {
        let store = build_pipeline_store();
        let config = PipelineConfig {
            limit: 20,
            max_neighbors_per_seed: 5,
            ..Default::default()
        };
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();

        // SqliteStore should be found either as seed (path match on "store") or context
        let all_ids: Vec<&str> = plan.items.iter().map(|i| i.entity_id.as_str()).collect();
        assert!(
            all_ids.contains(&"symbol::SqliteStore"),
            "SqliteStore should appear (as seed or context), got: {all_ids:?}"
        );
    }

    #[test]
    fn pipeline_respects_limit() {
        let store = build_pipeline_store();
        let config = PipelineConfig {
            limit: 2,
            ..Default::default()
        };
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();
        assert!(plan.items.len() <= 2);
    }

    #[test]
    fn pipeline_items_sorted_by_score() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();

        for w in plan.items.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "items should be sorted descending: {} >= {}",
                w[0].score,
                w[1].score
            );
        }
    }

    #[test]
    fn pipeline_no_duplicate_entity_ids() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();

        let mut ids: Vec<&str> = plan.items.iter().map(|i| i.entity_id.as_str()).collect();
        let count_before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(
            ids.len(),
            count_before,
            "should have no duplicate entity_ids"
        );
    }

    #[test]
    fn pipeline_items_have_reasons() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();

        for item in &plan.items {
            assert!(
                !item.reasons.is_empty(),
                "item {} should have reasons",
                item.entity_id
            );
        }
    }

    #[test]
    fn pipeline_candidates_considered_nonzero() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();
        assert!(plan.candidates_considered > 0);
    }

    #[test]
    fn pipeline_empty_query_returns_general() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "", None, &config).unwrap();
        assert_eq!(plan.category, "general");
        // Empty query tokens -> no keyword matching, no routes for general -> empty
        assert!(plan.items.is_empty());
    }

    #[test]
    fn pipeline_used_vector_search_flag() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let embedding = vec![0.1_f32; 128];

        let plan = QueryPipeline::run(&store, "store", Some(&embedding), &config).unwrap();
        assert!(plan.used_vector_search);
    }

    #[test]
    fn pipeline_used_vector_search_flag_none() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();

        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();
        assert!(!plan.used_vector_search);
    }

    #[test]
    fn pipeline_debug_query_uses_debug_routes() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "fix the entity bug", None, &config).unwrap();

        // debug route has symbol::Entity (pri 90)
        let ids: Vec<&str> = plan.items.iter().map(|i| i.entity_id.as_str()).collect();
        assert!(
            ids.contains(&"symbol::Entity"),
            "debug query should find Entity via debug route, got: {ids:?}"
        );
    }

    #[test]
    fn pipeline_json_round_trip() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "how does the store work", None, &config).unwrap();

        let json = serde_json::to_string(&plan).unwrap();
        let restored: crate::plan::ReadingPlan = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.query, plan.query);
        assert_eq!(restored.category, plan.category);
        assert_eq!(restored.items.len(), plan.items.len());
        assert_eq!(restored.candidates_considered, plan.candidates_considered);
        assert_eq!(restored.used_vector_search, plan.used_vector_search);

        for (orig, rest) in plan.items.iter().zip(restored.items.iter()) {
            assert_eq!(orig.entity_id, rest.entity_id);
            assert!((orig.score - rest.score).abs() < 1e-10);
            assert_eq!(orig.reasons, rest.reasons);
            assert_eq!(orig.is_context, rest.is_context);
            assert_eq!(orig.context_via, rest.context_via);
        }
    }

    #[test]
    fn pipeline_display_output_nonempty() {
        let store = build_pipeline_store();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();
        let display = plan.display();
        assert!(display.contains("Reading plan for:"));
        assert!(display.contains("store"));
    }

    #[test]
    fn pipeline_empty_store() {
        let store = Store::open_in_memory().unwrap();
        let config = PipelineConfig::default();
        let plan = QueryPipeline::run(&store, "anything", None, &config).unwrap();
        assert!(plan.items.is_empty());
        assert_eq!(plan.candidates_considered, 0);
    }

    #[test]
    fn pipeline_task_route_entity_scores_higher_than_keyword_only() {
        let store = build_pipeline_store();
        let config = PipelineConfig {
            limit: 20,
            ..Default::default()
        };
        // "understand" route points to component::store with priority 80
        // Query "how does the store work" -> category=understand, tokens include "store"
        let plan = QueryPipeline::run(&store, "how does the store work", None, &config).unwrap();

        let store_item = plan
            .items
            .iter()
            .find(|i| i.entity_id == "component::store");
        let engine_item = plan
            .items
            .iter()
            .find(|i| i.entity_id == "source_unit::engine");

        // component::store has task_route + name/path/keyword matches
        // source_unit::engine has only name match on "engine" which doesn't match any query token
        // So store should score higher than engine (if engine is even present)
        if let (Some(s), Some(e)) = (store_item, engine_item) {
            assert!(
                s.score > e.score,
                "store ({}) should outscore engine ({})",
                s.score,
                e.score
            );
        }
    }

    #[test]
    fn pipeline_category_override() {
        let store = build_pipeline_store();
        let config = PipelineConfig {
            category_override: Some(TaskCategory::Deploy),
            ..Default::default()
        };
        // "store" would normally classify as General, but override forces Deploy
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();
        assert_eq!(plan.category, "deploy");
    }

    #[test]
    fn pipeline_category_override_none_uses_heuristic() {
        let store = build_pipeline_store();
        let config = PipelineConfig {
            category_override: None,
            ..Default::default()
        };
        let plan = QueryPipeline::run(&store, "fix the entity bug", None, &config).unwrap();
        assert_eq!(plan.category, "debug");
    }

    #[test]
    fn pipeline_context_items_have_context_via() {
        let store = build_pipeline_store();
        let config = PipelineConfig {
            limit: 20,
            max_neighbors_per_seed: 5,
            ..Default::default()
        };
        let plan = QueryPipeline::run(&store, "store", None, &config).unwrap();

        for item in &plan.items {
            if item.is_context {
                assert!(
                    item.context_via.is_some(),
                    "context item {} should have context_via",
                    item.entity_id
                );
            } else {
                assert!(
                    item.context_via.is_none(),
                    "non-context item {} should not have context_via",
                    item.entity_id
                );
            }
        }
    }
}
