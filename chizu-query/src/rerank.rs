use chizu_core::{RerankWeights, TaskCategory};

use crate::retrieval::Candidate;

pub fn score(candidates: &mut [Candidate], category: TaskCategory, weights: &RerankWeights) {
    let preferred = category.preferred_kinds();

    for candidate in candidates.iter_mut() {
        let task_route_norm = candidate
            .task_route_priority
            .map(|p| p as f64 / 100.0)
            .unwrap_or(0.0);

        let kind_str = candidate.entity.kind.to_string();
        let kind_boost = if preferred.contains(&kind_str.as_str()) {
            1.0
        } else {
            0.0
        };

        let exported_boost = if candidate.entity.exported { 1.0 } else { 0.0 };

        let mut score = weights.task_route * task_route_norm
            + weights.keyword * candidate.keyword_score
            + weights.name_match * candidate.name_match_score
            + weights.vector * candidate.vector_score
            + weights.kind_preference * kind_boost
            + weights.exported * exported_boost
            + weights.path_match * candidate.path_match_score;

        if candidate.is_context {
            score *= 0.5;
        }

        candidate.final_score = score;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::Candidate;
    use chizu_core::{Entity, EntityKind, RerankWeights, TaskCategory};

    fn make_candidate(
        id: &str,
        kind: EntityKind,
        keyword: f64,
        name: f64,
        vector: f64,
        exported: bool,
        is_context: bool,
    ) -> Candidate {
        Candidate {
            entity: Entity::new(id, kind, "name").with_exported(exported),
            task_route_priority: None,
            keyword_score: keyword,
            name_match_score: name,
            path_match_score: 0.0,
            vector_score: vector,
            is_context,
            final_score: 0.0,
        }
    }

    #[test]
    fn test_rerank_sorts_correctly() {
        let weights = RerankWeights {
            task_route: 0.0,
            keyword: 0.25,
            name_match: 0.25,
            vector: 0.25,
            kind_preference: 0.0,
            exported: 0.0,
            path_match: 0.0,
        };

        let mut candidates = vec![
            make_candidate("a", EntityKind::Test, 1.0, 0.0, 0.0, false, false),
            make_candidate("b", EntityKind::Test, 0.0, 1.0, 0.0, false, false),
            make_candidate("c", EntityKind::Test, 0.0, 0.0, 1.0, false, false),
        ];

        score(&mut candidates, TaskCategory::General, &weights);

        // All signals have same weight, so scores should be equal.
        assert!((candidates[0].final_score - candidates[1].final_score).abs() < 0.001);
        assert!((candidates[1].final_score - candidates[2].final_score).abs() < 0.001);
    }

    #[test]
    fn test_context_discount() {
        let weights = RerankWeights {
            task_route: 0.0,
            keyword: 1.0,
            name_match: 0.0,
            vector: 0.0,
            kind_preference: 0.0,
            exported: 0.0,
            path_match: 0.0,
        };

        let mut candidates = vec![
            make_candidate("a", EntityKind::Symbol, 1.0, 0.0, 0.0, false, false),
            make_candidate("b", EntityKind::Symbol, 1.0, 0.0, 0.0, false, true),
        ];

        score(&mut candidates, TaskCategory::General, &weights);

        assert!((candidates[0].final_score - 1.0).abs() < 0.001);
        assert!((candidates[1].final_score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_kind_boost() {
        let weights = RerankWeights {
            task_route: 0.0,
            keyword: 0.0,
            name_match: 0.0,
            vector: 0.0,
            kind_preference: 1.0,
            exported: 0.0,
            path_match: 0.0,
        };

        let mut candidates = vec![
            make_candidate("a", EntityKind::Component, 0.0, 0.0, 0.0, false, false),
            make_candidate("b", EntityKind::Test, 0.0, 0.0, 0.0, false, false),
        ];

        score(&mut candidates, TaskCategory::Understand, &weights);

        assert!(candidates[0].final_score > candidates[1].final_score);
        assert!((candidates[0].final_score - 1.0).abs() < 0.001);
        assert!((candidates[1].final_score - 0.0).abs() < 0.001);
    }
}
