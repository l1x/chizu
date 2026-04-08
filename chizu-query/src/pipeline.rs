use std::time::Instant;

use chizu_core::{
    Config, CutoffMode, Provider, RerankDocument, Reranker, Store, TaskCategory, classify_query,
};

use crate::cutoff;
use crate::error::Result;
use crate::expansion;
use crate::plan::{PipelineTimings, PlanEntry, ReadingPlan};
use crate::rerank;
use crate::retrieval;

/// Options controlling search behavior beyond configuration.
pub struct SearchOptions {
    /// Maximum number of results.
    pub limit: usize,
    /// Bypass cutoff and return up to `limit` results.
    pub show_all: bool,
    /// Include per-signal score breakdowns and timings in output.
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
        reranker: Option<&dyn Reranker>,
    ) -> Result<ReadingPlan> {
        let pipeline_start = Instant::now();
        let category = category.unwrap_or_else(|| classify_query(query));

        // Retrieve
        let t0 = Instant::now();
        let mut candidates = retrieval::retrieve(store, query, category, config, provider)?;
        let retrieval_ms = t0.elapsed().as_millis() as u64;

        // Initial scoring for expansion seed selection
        let t0 = Instant::now();
        rerank::score(&mut candidates, category, &config.search.rerank_weights);
        let scoring_ms_1 = t0.elapsed().as_millis() as u64;

        // Expand
        let t0 = Instant::now();
        expansion::expand(store, &mut candidates, options.limit)?;
        let expansion_ms = t0.elapsed().as_millis() as u64;

        // Final first-stage scoring
        let t0 = Instant::now();
        rerank::score(&mut candidates, category, &config.search.rerank_weights);
        let scoring_ms_2 = t0.elapsed().as_millis() as u64;

        candidates.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Second-stage reranking (replaces ordering, does not blend scores)
        let reranking_ms = if config.reranker.enabled {
            if let Some(reranker) = reranker {
                let t0 = Instant::now();
                let top_k = config.reranker.top_k.min(candidates.len());
                match apply_reranker(store, query, &mut candidates, top_k, reranker) {
                    Ok(()) => {
                        tracing::debug!("reranking applied to top {} candidates", top_k);
                    }
                    Err(e) => {
                        tracing::warn!("reranker failed, falling back to first-stage order: {e}");
                    }
                }
                Some(t0.elapsed().as_millis() as u64)
            } else {
                tracing::debug!("reranker enabled but no reranker instance provided");
                None
            }
        } else {
            None
        };

        // Cutoff
        let t0 = Instant::now();
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
        let cutoff_ms = t0.elapsed().as_millis() as u64;

        let cutoff_applied = !options.show_all
            && config.search.cutoff != CutoffMode::None
            && effective_limit < total_before_cutoff;

        let total_ms = pipeline_start.elapsed().as_millis() as u64;

        let timings = if options.verbose {
            Some(PipelineTimings {
                retrieval_ms,
                scoring_ms: scoring_ms_1 + scoring_ms_2,
                expansion_ms,
                reranking_ms,
                cutoff_ms,
                total_ms,
            })
        } else {
            None
        };

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
            timings,
        })
    }
}

/// Apply second-stage reranking to the top_k candidates.
///
/// Fetches summaries to build document text, calls the reranker, then
/// reorders candidates by reranker score (replacing first-stage ordering).
fn apply_reranker(
    store: &dyn Store,
    query: &str,
    candidates: &mut Vec<retrieval::Candidate>,
    top_k: usize,
    reranker: &dyn Reranker,
) -> Result<()> {
    if top_k == 0 || candidates.is_empty() {
        return Ok(());
    }

    let top_k = top_k.min(candidates.len());

    let mut documents = Vec::with_capacity(top_k);
    for candidate in candidates.iter().take(top_k) {
        let mut text = format!("Name: {}", candidate.entity.name);
        if let Some(ref path) = candidate.entity.path {
            text.push_str(&format!("\nPath: {path}"));
        }
        if let Ok(Some(summary)) = store.get_summary(&candidate.entity.id) {
            text.push_str(&format!("\nSummary: {}", summary.short_summary));
            if let Some(ref kw) = summary.keywords {
                text.push_str(&format!("\nKeywords: {}", kw.join(", ")));
            }
        }
        documents.push(RerankDocument { text });
    }

    let mut scores = reranker
        .rerank(query, &documents)
        .map_err(|e| crate::error::QueryError::Other(e.to_string()))?;

    // Sort by descending score (trait returns unsorted)
    scores.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Build the desired order: reranked top_k first, then the tail
    let mut tail: Vec<retrieval::Candidate> = candidates.split_off(top_k);
    let mut pool = std::mem::take(candidates);

    // Place candidates in reranker-determined order with replaced scores
    for rs in &scores {
        if rs.index < pool.len() {
            pool[rs.index].final_score = rs.score;
        }
    }
    let order: Vec<usize> = scores.iter().map(|s| s.index).collect();
    for idx in order {
        if idx < pool.len() {
            // swap-remove is O(1) but changes indices — safe since we use
            // original indices from reranker and only visit each once
            candidates.push(std::mem::replace(
                &mut pool[idx],
                retrieval::Candidate::placeholder(""),
            ));
        }
    }
    // Append any candidates not returned by the reranker (defensive)
    for c in pool {
        if !c.entity.id.is_empty() {
            candidates.push(c);
        }
    }
    candidates.append(&mut tail);

    Ok(())
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
            SearchPipeline::run(&store, "auth debug", None, &options, &config, None, None).unwrap();

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
        let plan =
            SearchPipeline::run(&store, "common", None, &options, &config, None, None).unwrap();

        assert_eq!(plan.entries.len(), 3);
        assert_eq!(plan.entries[0].rank, 1);
        assert_eq!(plan.entries[2].rank, 3);
    }

    #[test]
    fn test_pipeline_cutoff() {
        let (store, _temp) = create_test_store();

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
        let plan =
            SearchPipeline::run(&store, "auth", None, &options, &config, None, None).unwrap();

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
        let plan =
            SearchPipeline::run(&store, "common", None, &options, &config, None, None).unwrap();

        assert!(plan.total_before_cutoff.is_none());
    }

    #[test]
    fn test_pipeline_verbose_includes_breakdown_and_timings() {
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
        let plan =
            SearchPipeline::run(&store, "handler", None, &options, &config, None, None).unwrap();

        assert!(!plan.entries.is_empty());
        assert!(plan.entries[0].score_breakdown.is_some());
        assert!(plan.timings.is_some());
        let t = plan.timings.unwrap();
        assert!(t.total_ms >= t.retrieval_ms);
        assert!(t.reranking_ms.is_none()); // Reranker not enabled
    }

    #[test]
    fn test_pipeline_reranker_disabled_by_default() {
        let (store, _temp) = create_test_store();

        store
            .insert_entity(&Entity::new("a", EntityKind::Symbol, "foo"))
            .unwrap();
        store
            .insert_summary(&Summary::new("a", "A function"))
            .unwrap();

        let config = Config::default();
        assert!(!config.reranker.enabled);

        let options = SearchOptions {
            limit: 5,
            show_all: false,
            verbose: true,
        };
        let plan = SearchPipeline::run(&store, "foo", None, &options, &config, None, None).unwrap();

        let t = plan.timings.unwrap();
        assert!(t.reranking_ms.is_none());
    }
}
