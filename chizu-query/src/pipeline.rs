use chizu_core::{Config, Provider, Store, TaskCategory, classify_query};

use crate::error::Result;
use crate::expansion;
use crate::plan::{PlanEntry, ReadingPlan};
use crate::rerank;
use crate::retrieval;

pub struct SearchPipeline;

impl SearchPipeline {
    pub fn run(
        store: &dyn Store,
        query: &str,
        category: Option<TaskCategory>,
        limit: usize,
        config: &Config,
        provider: Option<&dyn Provider>,
    ) -> Result<ReadingPlan> {
        let category = category.unwrap_or_else(|| classify_query(query));

        // 1. Retrieve candidates from all sources
        let mut candidates = retrieval::retrieve(store, query, category, config, provider)?;

        // 2. Initial scoring for expansion seed selection
        rerank::score(&mut candidates, category, &config.search.rerank_weights);

        // 3. Expand with 1-hop neighbors
        expansion::expand(store, &mut candidates, limit)?;

        // 4. Final reranking after expansion
        rerank::score(&mut candidates, category, &config.search.rerank_weights);

        // 5. Sort and truncate
        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(limit);

        // 6. Build reading plan
        let entries: Vec<PlanEntry> = candidates
            .into_iter()
            .enumerate()
            .map(|(rank, c)| PlanEntry {
                rank: rank + 1,
                entity_id: c.entity.id.clone(),
                entity_kind: c.entity.kind,
                name: c.entity.name.clone(),
                path: c.entity.path.clone(),
                score: c.final_score,
                is_context: c.is_context,
                reasons: build_reasons(&c),
            })
            .collect();

        Ok(ReadingPlan {
            query: query.to_string(),
            category,
            entries,
        })
    }
}

fn build_reasons(candidate: &retrieval::Candidate) -> Vec<String> {
    let mut reasons = Vec::new();
    if candidate.task_route_priority.is_some() {
        reasons.push("task route".to_string());
    }
    if candidate.keyword_score > 0.0 {
        reasons.push("keyword match".to_string());
    }
    if candidate.name_match_score > 0.0 {
        reasons.push("name match".to_string());
    }
    if candidate.path_match_score > 0.0 {
        reasons.push("path match".to_string());
    }
    if candidate.vector_score > 0.0 {
        reasons.push("semantic similarity".to_string());
    }
    if candidate.is_context {
        reasons.push("graph neighbor".to_string());
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{
        ChizuStore, Config, Edge, EdgeKind, Entity, EntityKind, Store, Summary, TaskRoute,
    };
    use tempfile::TempDir;

    fn create_test_store() -> (ChizuStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let store = ChizuStore::open(temp_dir.path(), &config).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_pipeline_end_to_end() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("a", EntityKind::Symbol, "auth_handler"))
            .unwrap();
        store
            .insert_entity(&Entity::new("b", EntityKind::Symbol, "helper"))
            .unwrap();
        store
            .insert_task_route(&TaskRoute::new("debug", "a", 80))
            .unwrap();
        store
            .insert_summary(&Summary::new("a", "Handles authentication"))
            .unwrap();
        store
            .insert_edge(&Edge::new("a", EdgeKind::Defines, "b"))
            .unwrap();

        let config = Config::default();
        let plan = SearchPipeline::run(&store, "auth debug", None, 5, &config, None).unwrap();

        assert_eq!(plan.category, TaskCategory::Debug);
        assert!(!plan.entries.is_empty());

        let top = &plan.entries[0];
        assert_eq!(top.entity_id, "a");
        assert!(top.score > 0.0);
    }

    #[test]
    fn test_pipeline_respects_limit() {
        let (store, _temp) = create_test_store();

        for i in 0..10 {
            let id = format!("e{}", i);
            store
                .insert_entity(&Entity::new(&id, EntityKind::Symbol, &id))
                .unwrap();
            store
                .insert_summary(&Summary::new(&id, "common term"))
                .unwrap();
        }

        let config = Config::default();
        let plan = SearchPipeline::run(&store, "common", None, 3, &config, None).unwrap();

        assert_eq!(plan.entries.len(), 3);
        assert_eq!(plan.entries[0].rank, 1);
        assert_eq!(plan.entries[2].rank, 3);
    }
}
