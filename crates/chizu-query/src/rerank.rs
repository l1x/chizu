use crate::classify::TaskCategory;
use crate::retrieve::{Candidate, RetrievalSource};

const CONTEXT_DISCOUNT: f64 = 0.50;

/// Weights for reranking signals.
/// These determine how much each signal contributes to the final score.
#[derive(Debug, Clone, Copy)]
pub struct RerankWeights {
    pub task_route: f64,
    pub keyword: f64,
    pub name_match: f64,
    pub vector: f64,
    pub kind_preference: f64,
    pub exported: f64,
    pub path_match: f64,
}

impl Default for RerankWeights {
    fn default() -> Self {
        Self {
            // Task route weight set to 0 until task routes are fully implemented.
            // Currently no runtime producer generates TaskRoute records.
            // TODO: Re-enable (suggest 0.30) when task route generation is implemented.
            task_route: 0.0,
            // Redistribute the 0.30 from task route evenly across active signals
            keyword: 0.25,
            name_match: 0.20,
            vector: 0.25,
            kind_preference: 0.10,
            exported: 0.10,
            path_match: 0.10,
        }
    }
}

impl RerankWeights {
    /// Verify that weights sum to approximately 1.0
    pub fn is_valid(&self) -> bool {
        let sum = self.task_route
            + self.keyword
            + self.name_match
            + self.vector
            + self.kind_preference
            + self.exported
            + self.path_match;
        (sum - 1.0).abs() < 0.001
    }
}

/// A scored entry ready for ranking.
#[derive(Debug, Clone)]
pub struct ScoredEntry {
    pub candidate: Candidate,
    pub score: f64,
    pub reasons: Vec<String>,
    pub is_context: bool,
    pub context_via: Option<String>,
}

/// Score a single candidate.
fn score_candidate(
    candidate: &Candidate,
    category: &TaskCategory,
    query_tokens: &[String],
    weights: &RerankWeights,
) -> (f64, Vec<String>) {
    let mut score = 0.0;
    let mut reasons = Vec::new();

    // Task route priority signal
    for src in &candidate.sources {
        if let RetrievalSource::TaskRoute { priority } = src {
            // Normalize priority: higher priority = higher score. Cap at 100.
            let norm = (*priority as f64).min(100.0) / 100.0;
            score += weights.task_route * norm;
            reasons.push(format!("task_route(priority={})", priority));
        }
    }

    // Keyword match signal
    if candidate
        .sources
        .iter()
        .any(|s| matches!(s, RetrievalSource::KeywordMatch))
    {
        score += weights.keyword;
        reasons.push("keyword_match".to_string());
    } else if !candidate.keywords.is_empty() && !query_tokens.is_empty() {
        // Check keyword overlap even if not the original retrieval source
        let kw_lower: Vec<String> = candidate
            .keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();
        let overlap = query_tokens
            .iter()
            .filter(|t| kw_lower.iter().any(|k| k.contains(t.as_str())))
            .count();
        if overlap > 0 {
            let frac = overlap as f64 / query_tokens.len().max(1) as f64;
            score += weights.keyword * frac;
            reasons.push(format!(
                "keyword_overlap({}/{})",
                overlap,
                query_tokens.len()
            ));
        }
    }

    // Name match signal
    if candidate
        .sources
        .iter()
        .any(|s| matches!(s, RetrievalSource::NameMatch))
    {
        score += weights.name_match;
        reasons.push("name_match".to_string());
    } else {
        let name_lower = candidate.entity.name.to_lowercase();
        if query_tokens.iter().any(|t| name_lower.contains(t.as_str())) {
            score += weights.name_match;
            reasons.push("name_contains_term".to_string());
        }
    }

    // Vector similarity signal
    for src in &candidate.sources {
        if let RetrievalSource::VectorSearch { distance } = src {
            let sim = 1.0 - (*distance as f64) / 2.0;
            score += weights.vector * sim.max(0.0);
            reasons.push(format!("vector(dist={:.3})", distance));
        }
    }

    // Kind preference signal
    let preferred = category.preferred_kinds();
    if preferred.contains(&candidate.entity.kind) {
        score += weights.kind_preference;
        reasons.push("kind_preferred".to_string());
    }

    // Exported bonus
    if candidate.entity.exported {
        score += weights.exported;
        reasons.push("exported".to_string());
    }

    // Path match signal
    if candidate
        .sources
        .iter()
        .any(|s| matches!(s, RetrievalSource::PathMatch))
    {
        score += weights.path_match;
        reasons.push("path_match".to_string());
    } else {
        let path_lower = candidate
            .entity
            .path
            .as_deref()
            .unwrap_or("")
            .to_lowercase();
        if !path_lower.is_empty() && query_tokens.iter().any(|t| path_lower.contains(t.as_str())) {
            score += weights.path_match;
            reasons.push("path_contains_term".to_string());
        }
    }

    (score, reasons)
}

/// Rerank seed candidates and context neighbors into a scored, sorted list.
pub fn rerank(
    seeds: Vec<Candidate>,
    neighbors: Vec<(Candidate, String)>,
    category: &TaskCategory,
    query_tokens: &[String],
    limit: usize,
    weights: &RerankWeights,
) -> Vec<ScoredEntry> {
    let mut entries: Vec<ScoredEntry> = Vec::new();

    // Score seeds
    for candidate in seeds {
        let (score, reasons) = score_candidate(&candidate, category, query_tokens, weights);
        entries.push(ScoredEntry {
            candidate,
            score,
            reasons,
            is_context: false,
            context_via: None,
        });
    }

    // Score neighbors with context discount
    for (candidate, via) in neighbors {
        let (raw_score, reasons) = score_candidate(&candidate, category, query_tokens, weights);
        entries.push(ScoredEntry {
            candidate,
            score: raw_score * CONTEXT_DISCOUNT,
            reasons,
            is_context: true,
            context_via: Some(via),
        });
    }

    // Sort descending by score
    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Dedup by entity_id (keep higher score)
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert(e.candidate.entity.id.clone()));

    entries.truncate(limit);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::model::{Entity, EntityKind};

    const EPS: f64 = 1e-9;

    fn make_entity(
        id: &str,
        name: &str,
        kind: EntityKind,
        path: Option<&str>,
        exported: bool,
    ) -> Entity {
        Entity {
            id: id.to_string(),
            kind,
            name: name.to_string(),
            component_id: None,
            path: path.map(String::from),
            language: Some("rust".to_string()),
            line_start: Some(1),
            line_end: Some(50),
            visibility: if exported {
                Some("pub".to_string())
            } else {
                None
            },
            exported,
        }
    }

    fn make_candidate(
        entity: Entity,
        sources: Vec<RetrievalSource>,
        keywords: Vec<&str>,
    ) -> Candidate {
        Candidate {
            entity,
            sources,
            short_summary: String::new(),
            keywords: keywords.into_iter().map(String::from).collect(),
        }
    }

    // --- Individual signal weight tests ---

    #[test]
    fn score_task_route_signal() {
        let entity = make_entity("e::A", "Unrelated", EntityKind::Repo, None, false);
        let candidate = make_candidate(
            entity,
            vec![RetrievalSource::TaskRoute { priority: 100 }],
            vec![],
        );
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        // priority=100 -> norm=1.0 -> W_TASK_ROUTE * 1.0 = 0.30
        assert!((result[0].score - W_TASK_ROUTE).abs() < EPS);
        assert!(result[0].reasons.iter().any(|r| r.contains("task_route")));
    }

    #[test]
    fn score_task_route_capped_at_100() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(
            entity,
            vec![RetrievalSource::TaskRoute { priority: 200 }],
            vec![],
        );
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        // priority=200, capped to 100 -> norm=1.0 -> same as 100
        assert!((result[0].score - W_TASK_ROUTE).abs() < EPS);
    }

    #[test]
    fn score_task_route_partial_priority() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(
            entity,
            vec![RetrievalSource::TaskRoute { priority: 50 }],
            vec![],
        );
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        // priority=50 -> norm=0.5 -> 0.30 * 0.5 = 0.15
        assert!((result[0].score - W_TASK_ROUTE * 0.5).abs() < EPS);
    }

    #[test]
    fn score_keyword_match_source() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(entity, vec![RetrievalSource::KeywordMatch], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        assert!((result[0].score - W_KEYWORD).abs() < EPS);
        assert!(result[0].reasons.contains(&"keyword_match".to_string()));
    }

    #[test]
    fn score_keyword_overlap_fraction() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        // No KeywordMatch source, but keywords overlap with query tokens
        let candidate = make_candidate(entity, vec![], vec!["engine", "store", "other"]);
        let tokens = vec!["engine".to_string(), "parser".to_string()];
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &tokens, 10);
        // 1 overlap out of 2 tokens -> frac = 0.5 -> W_KEYWORD * 0.5 = 0.10
        assert!((result[0].score - W_KEYWORD * 0.5).abs() < EPS);
        assert!(result[0]
            .reasons
            .iter()
            .any(|r| r.contains("keyword_overlap(1/2)")));
    }

    #[test]
    fn score_keyword_overlap_all_match() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(entity, vec![], vec!["engine", "parser"]);
        let tokens = vec!["engine".to_string(), "parser".to_string()];
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &tokens, 10);
        // 2/2 -> frac = 1.0 -> full W_KEYWORD
        assert!((result[0].score - W_KEYWORD).abs() < EPS);
    }

    #[test]
    fn score_name_match_source() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(entity, vec![RetrievalSource::NameMatch], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        assert!((result[0].score - W_NAME_MATCH).abs() < EPS);
        assert!(result[0].reasons.contains(&"name_match".to_string()));
    }

    #[test]
    fn score_name_contains_term_fallback() {
        let entity = make_entity("e::A", "EngineCore", EntityKind::Repo, None, false);
        // No NameMatch source, but name contains query term
        let candidate = make_candidate(entity, vec![], vec![]);
        let tokens = vec!["engine".to_string()];
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &tokens, 10);
        assert!((result[0].score - W_NAME_MATCH).abs() < EPS);
        assert!(result[0]
            .reasons
            .contains(&"name_contains_term".to_string()));
    }

    #[test]
    fn score_vector_similarity() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(
            entity,
            vec![RetrievalSource::VectorSearch { distance: 0.4 }],
            vec![],
        );
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        // sim = 1.0 - 0.4/2.0 = 0.8 -> W_VECTOR * 0.8 = 0.20 * 0.8 = 0.16
        let expected = W_VECTOR * 0.8;
        assert!((result[0].score - expected).abs() < EPS);
        assert!(result[0].reasons.iter().any(|r| r.contains("vector")));
    }

    #[test]
    fn score_vector_large_distance_clamped() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(
            entity,
            vec![RetrievalSource::VectorSearch { distance: 3.0 }],
            vec![],
        );
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        // sim = 1.0 - 3.0/2.0 = -0.5, clamped to 0.0 -> W_VECTOR * 0.0 = 0.0
        assert!(result[0].score.abs() < EPS);
    }

    #[test]
    fn score_vector_zero_distance() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(
            entity,
            vec![RetrievalSource::VectorSearch { distance: 0.0 }],
            vec![],
        );
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        // sim = 1.0 - 0.0/2.0 = 1.0 -> W_VECTOR * 1.0 = 0.20
        assert!((result[0].score - W_VECTOR).abs() < EPS);
    }

    #[test]
    fn score_kind_preference() {
        // Symbol is preferred for Understand
        let entity = make_entity("e::A", "X", EntityKind::Symbol, None, false);
        let candidate = make_candidate(entity, vec![], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::Understand, &[], 10);
        assert!((result[0].score - W_KIND_PREF).abs() < EPS);
        assert!(result[0].reasons.contains(&"kind_preferred".to_string()));
    }

    #[test]
    fn score_kind_not_preferred() {
        // Bench is NOT preferred for Understand
        let entity = make_entity("e::A", "X", EntityKind::Bench, None, false);
        let candidate = make_candidate(entity, vec![], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::Understand, &[], 10);
        assert!(result[0].score.abs() < EPS);
        assert!(!result[0].reasons.contains(&"kind_preferred".to_string()));
    }

    #[test]
    fn score_exported_bonus() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, true);
        let candidate = make_candidate(entity, vec![], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        assert!((result[0].score - W_EXPORTED).abs() < EPS);
        assert!(result[0].reasons.contains(&"exported".to_string()));
    }

    #[test]
    fn score_not_exported() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(entity, vec![], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        assert!(result[0].score.abs() < EPS);
    }

    #[test]
    fn score_path_match_source() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(entity, vec![RetrievalSource::PathMatch], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        assert!((result[0].score - W_PATH_MATCH).abs() < EPS);
        assert!(result[0].reasons.contains(&"path_match".to_string()));
    }

    #[test]
    fn score_path_contains_term_fallback() {
        let entity = make_entity(
            "e::A",
            "X",
            EntityKind::Repo,
            Some("src/engine/core.rs"),
            false,
        );
        let candidate = make_candidate(entity, vec![], vec![]);
        let tokens = vec!["engine".to_string()];
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &tokens, 10);
        assert!((result[0].score - W_PATH_MATCH).abs() < EPS);
        assert!(result[0]
            .reasons
            .contains(&"path_contains_term".to_string()));
    }

    #[test]
    fn score_no_path_no_path_bonus() {
        let entity = make_entity("e::A", "X", EntityKind::Repo, None, false);
        let candidate = make_candidate(entity, vec![], vec![]);
        let tokens = vec!["engine".to_string()];
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &tokens, 10);
        // No path -> path_contains_term should not fire (empty path check)
        assert!(!result[0].reasons.iter().any(|r| r.contains("path")));
    }

    // --- Combined signals ---

    #[test]
    fn score_all_signals_combined() {
        let entity = make_entity(
            "e::A",
            "Engine",
            EntityKind::Symbol,
            Some("src/engine.rs"),
            true,
        );
        let candidate = make_candidate(
            entity,
            vec![
                RetrievalSource::TaskRoute { priority: 100 },
                RetrievalSource::KeywordMatch,
                RetrievalSource::NameMatch,
                RetrievalSource::VectorSearch { distance: 0.0 },
                RetrievalSource::PathMatch,
            ],
            vec![],
        );
        let result = rerank(
            vec![candidate],
            vec![],
            &TaskCategory::Understand, // Symbol is preferred
            &[],
            10,
        );
        // W_TASK_ROUTE(0.30) + W_KEYWORD(0.20) + W_NAME_MATCH(0.15) + W_VECTOR(0.20) + W_KIND_PREF(0.05) + W_EXPORTED(0.05) + W_PATH_MATCH(0.05) = 1.00
        assert!((result[0].score - 1.0).abs() < EPS);
    }

    // --- Context discount ---

    #[test]
    fn context_discount_applied_to_neighbors() {
        // Use Repo kind (NOT in General's preferred_kinds) to isolate the discount signal
        let entity = make_entity("e::A", "Engine", EntityKind::Repo, None, true);
        let candidate = make_candidate(entity, vec![RetrievalSource::NameMatch], vec![]);
        let tokens = vec![];
        let result = rerank(
            vec![],
            vec![(candidate, "e::Seed".to_string())],
            &TaskCategory::General,
            &tokens,
            10,
        );
        // NameMatch(0.15) + Exported(0.05) = 0.20, then * 0.50 = 0.10
        let expected = (W_NAME_MATCH + W_EXPORTED) * CONTEXT_DISCOUNT;
        assert!((result[0].score - expected).abs() < EPS);
        assert!(result[0].is_context);
        assert_eq!(result[0].context_via.as_deref(), Some("e::Seed"));
    }

    #[test]
    fn seeds_not_discounted() {
        // Use Repo kind (NOT in General's preferred_kinds) to isolate the seed signal
        let entity = make_entity("e::A", "Engine", EntityKind::Repo, None, true);
        let candidate = make_candidate(entity, vec![RetrievalSource::NameMatch], vec![]);
        let result = rerank(vec![candidate], vec![], &TaskCategory::General, &[], 10);
        let expected = W_NAME_MATCH + W_EXPORTED;
        assert!((result[0].score - expected).abs() < EPS);
        assert!(!result[0].is_context);
        assert!(result[0].context_via.is_none());
    }

    // --- Sorting ---

    #[test]
    fn rerank_sorts_descending() {
        let high = make_candidate(
            make_entity("e::H", "H", EntityKind::Repo, None, true),
            vec![RetrievalSource::KeywordMatch, RetrievalSource::NameMatch],
            vec![],
        );
        let low = make_candidate(
            make_entity("e::L", "L", EntityKind::Repo, None, false),
            vec![],
            vec![],
        );
        let result = rerank(vec![low, high], vec![], &TaskCategory::General, &[], 10);
        assert!(result[0].score >= result[1].score);
        assert_eq!(result[0].candidate.entity.id, "e::H");
    }

    // --- Dedup ---

    #[test]
    fn rerank_dedup_keeps_higher_score() {
        // Same entity appears as both seed (full score) and neighbor (discounted)
        let seed_entity = make_entity("e::A", "Engine", EntityKind::Symbol, None, true);
        let neighbor_entity = seed_entity.clone();
        let seed = make_candidate(seed_entity, vec![RetrievalSource::NameMatch], vec![]);
        let neighbor = make_candidate(neighbor_entity, vec![RetrievalSource::NameMatch], vec![]);

        let result = rerank(
            vec![seed],
            vec![(neighbor, "e::X".to_string())],
            &TaskCategory::General,
            &[],
            10,
        );

        // Should only appear once
        assert_eq!(result.len(), 1);
        // Should keep the seed (higher score, not discounted)
        assert!(!result[0].is_context);
    }

    // --- Limit ---

    #[test]
    fn rerank_respects_limit() {
        let candidates: Vec<Candidate> = (0..10)
            .map(|i| {
                make_candidate(
                    make_entity(
                        &format!("e::{i}"),
                        &format!("E{i}"),
                        EntityKind::Repo,
                        None,
                        false,
                    ),
                    vec![],
                    vec![],
                )
            })
            .collect();
        let result = rerank(candidates, vec![], &TaskCategory::General, &[], 3);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn rerank_limit_larger_than_entries() {
        let candidates = vec![make_candidate(
            make_entity("e::A", "A", EntityKind::Repo, None, false),
            vec![],
            vec![],
        )];
        let result = rerank(candidates, vec![], &TaskCategory::General, &[], 100);
        assert_eq!(result.len(), 1);
    }

    // --- Empty inputs ---

    #[test]
    fn rerank_empty_inputs() {
        let result = rerank(vec![], vec![], &TaskCategory::General, &[], 10);
        assert!(result.is_empty());
    }

    // --- Weights sum to 1.0 ---

    #[test]
    fn weight_constants_sum_to_one() {
        let sum = W_TASK_ROUTE
            + W_KEYWORD
            + W_NAME_MATCH
            + W_VECTOR
            + W_KIND_PREF
            + W_EXPORTED
            + W_PATH_MATCH;
        assert!(
            (sum - 1.0).abs() < EPS,
            "weights should sum to 1.0, got {sum}"
        );
    }
}
