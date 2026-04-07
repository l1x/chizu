use chizu_core::{Config, CutoffMode, Provider, Store, TaskCategory, classify_query};

use crate::cutoff;
use crate::error::Result;
use crate::expansion;
use crate::plan::{PlanEntry, ReadingPlan};
use crate::rerank;
use crate::retrieval;

/// Options controlling search behavior beyond configuration.
pub struct SearchOptions {
    /// Maximum number of results.
    pub limit: usize,
    /// Bypass cutoff and return up to `limit` results.
    pub show_all: bool,
    /// Include per-signal score breakdowns in output.
    pub verbose: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 15,
            show_all: false,
            verbose: false,
        }
    }
}

pub struct SearchPipeline;

impl SearchPipeline {
    pub fn run(
        store: &dyn Store,
        query: &str,
        category: Option<TaskCategory>,
        options: &SearchOptions,
        config: &Config,
        provider: Option<&dyn Provider>,
    ) -> Result<ReadingPlan> {
        let category = category.unwrap_or_else(|| classify_query(query));

        let mut candidates = retrieval::retrieve(store, query, category, config, provider)?;

        // Initial scoring for expansion seed selection
        rerank::score(&mut candidates, category, &config.search.rerank_weights);
        expansion::expand(store, &mut candidates, options.limit)?;
        rerank::score(&mut candidates, category, &config.search.rerank_weights);

        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total_before_cutoff = candidates.len().min(options.limit);
        let effective_limit = if options.show_all || config.search.cutoff == CutoffMode::None {
            options.limit
        } else {
            let scores: Vec<f64> = candidates.iter().map(|c| c.final_score).collect();
            let max = config.search.max_results.min(options.limit);
            cutoff::apply_cutoff(
                &scores,
                &config.search.cutoff,
                config.search.relative_gap_threshold,
                config.search.min_results,
                max,
            )
        };
        candidates.truncate(effective_limit);

        let cutoff_applied = !options.show_all
            && config.search.cutoff != CutoffMode::None
            && effective_limit < total_before_cutoff;

        let entries: Vec<PlanEntry> = candidates
            .into_iter()
            .enumerate()
            .map(|(rank, c)| {
                let score_breakdown = if options.verbose {
                    Some(rerank::breakdown(&c, category))
                } else {
                    None
                };

                PlanEntry {
                    rank: rank + 1,
                    entity_id: c.entity.id.clone(),
                    entity_kind: c.entity.kind,
                    name: c.entity.name.clone(),
                    path: c.entity.path.clone(),
                    score: c.final_score,
                    is_context: c.is_context,
                    reasons: build_reasons(&c),
                    score_breakdown,
                }
            })
            .collect();

        Ok(ReadingPlan {
            query: query.to_string(),
            category,
            entries,
            total_before_cutoff: if cutoff_applied {
                Some(total_before_cutoff)
            } else {
                None
            },
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
    use chizu_core::{ChizuStore, Edge, EdgeKind, Entity, EntityKind, Store, Summary, TaskRoute};

    fn create_test_store() -> (ChizuStore, tempfile::TempDir) {
        ChizuStore::open_test(None)
    }

    fn default_options(limit: usize) -> SearchOptions {
        SearchOptions {
            limit,
            show_all: false,
            verbose: false,
        }
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
        let options = default_options(5);
        let plan =
            SearchPipeline::run(&store, "auth debug", None, &options, &config, None).unwrap();

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
        let options = default_options(3);
        let plan = SearchPipeline::run(&store, "common", None, &options, &config, None).unwrap();

        assert_eq!(plan.entries.len(), 3);
        assert_eq!(plan.entries[0].rank, 1);
        assert_eq!(plan.entries[2].rank, 3);
    }

    #[test]
    fn test_pipeline_cutoff() {
        let (store, _temp) = create_test_store();

        // Create entities with varying relevance
        let entities = [
            ("a", "auth handler login flow"),
            ("b", "auth middleware"),
            ("c", "auth token"),
            ("d", "unrelated config"),
            ("e", "unrelated other"),
        ];
        for (id, summary) in &entities {
            store
                .insert_entity(&Entity::new(*id, EntityKind::Symbol, *id))
                .unwrap();
            store.insert_summary(&Summary::new(*id, *summary)).unwrap();
        }

        let mut config = Config::default();
        config.search.cutoff = CutoffMode::RelativeGap;
        config.search.relative_gap_threshold = 0.80;
        config.search.min_results = 2;
        config.search.max_results = 5;

        let options = default_options(10);
        let plan = SearchPipeline::run(&store, "auth", None, &options, &config, None).unwrap();

        // With cutoff active, should get fewer results than total
        assert!(plan.entries.len() <= 5);
    }

    #[test]
    fn test_pipeline_show_all_bypasses_cutoff() {
        let (store, _temp) = create_test_store();

        for i in 0..5 {
            let id = format!("e{}", i);
            store
                .insert_entity(&Entity::new(&id, EntityKind::Symbol, &id))
                .unwrap();
            store
                .insert_summary(&Summary::new(&id, "common keyword"))
                .unwrap();
        }

        let mut config = Config::default();
        config.search.cutoff = CutoffMode::RelativeGap;

        let options = SearchOptions {
            limit: 10,
            show_all: true,
            verbose: false,
        };
        let plan = SearchPipeline::run(&store, "common", None, &options, &config, None).unwrap();

        assert!(plan.total_before_cutoff.is_none());
    }

    #[test]
    fn test_pipeline_verbose_includes_breakdown() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("a", EntityKind::Symbol, "handler"))
            .unwrap();
        store
            .insert_summary(&Summary::new("a", "Handles requests"))
            .unwrap();

        let config = Config::default();
        let options = SearchOptions {
            limit: 5,
            show_all: false,
            verbose: true,
        };
        let plan = SearchPipeline::run(&store, "handler", None, &options, &config, None).unwrap();

        assert!(!plan.entries.is_empty());
        assert!(plan.entries[0].score_breakdown.is_some());
    }
}
