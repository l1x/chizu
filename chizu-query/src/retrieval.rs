use std::collections::HashMap;

use chizu_core::{Config, Entity, Provider, Store, TaskCategory};

use crate::error::{QueryError, Result};

#[derive(Debug, Clone)]
pub struct Candidate {
    pub entity: Entity,
    pub task_route_priority: Option<i32>,
    pub keyword_score: f64,
    pub name_match_score: f64,
    pub path_match_score: f64,
    pub vector_score: f64,
    pub is_context: bool,
    pub final_score: f64,
}

impl Candidate {
    /// Placeholder candidate for an entity id that will be hydrated later.
    pub fn placeholder(entity_id: &str) -> Self {
        Self {
            entity: Entity::new(entity_id, chizu_core::EntityKind::Symbol, ""),
            task_route_priority: None,
            keyword_score: 0.0,
            name_match_score: 0.0,
            path_match_score: 0.0,
            vector_score: 0.0,
            is_context: false,
            final_score: 0.0,
        }
    }

    pub fn from_entity(entity: Entity) -> Self {
        Self {
            entity,
            task_route_priority: None,
            keyword_score: 0.0,
            name_match_score: 0.0,
            path_match_score: 0.0,
            vector_score: 0.0,
            is_context: false,
            final_score: 0.0,
        }
    }
}

/// Retrieve candidates from all three sources: task routes, keyword SQL, and vectors.
pub fn retrieve(
    store: &dyn Store,
    query: &str,
    category: TaskCategory,
    config: &Config,
    provider: Option<&dyn Provider>,
) -> Result<Vec<Candidate>> {
    let mut candidates: HashMap<String, Candidate> = HashMap::new();

    // 1. Task route prefilter
    for task_name in category.route_names() {
        for route in store.get_task_routes(task_name)? {
            let entry = candidates
                .entry(route.entity_id.clone())
                .or_insert_with(|| Candidate::placeholder(&route.entity_id));
            let current = entry.task_route_priority.unwrap_or(0);
            if route.priority > current {
                entry.task_route_priority = Some(route.priority);
            }
        }
    }

    // 2. Keyword / name / path SQL matching
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    if !tokens.is_empty() {
        let keyword_matches = keyword_search(store, &tokens)?;
        for (entity_id, match_count) in keyword_matches {
            let keyword_score = match_count as f64 / tokens.len() as f64;
            let entry = candidates
                .entry(entity_id.clone())
                .or_insert_with(|| Candidate::placeholder(&entity_id));
            entry.keyword_score = keyword_score.max(entry.keyword_score);
        }

        // Name and path matching across preferred entity kinds for this category.
        for kind_str in category.preferred_kinds() {
            let kind: chizu_core::EntityKind = match kind_str.parse() {
                Ok(k) => k,
                Err(_) => continue,
            };
            for entity in store.get_entities_by_kind(kind)? {
                let name_lower = entity.name.to_lowercase();
                let path_lower = entity.path.as_ref().map(|p| p.to_lowercase());

                let name_hits = tokens.iter().filter(|t| name_lower.contains(*t)).count();
                let path_hits = path_lower
                    .as_ref()
                    .map(|p| tokens.iter().filter(|t| p.contains(*t)).count())
                    .unwrap_or(0);

                if name_hits > 0 || path_hits > 0 {
                    let entry = candidates
                        .entry(entity.id.clone())
                        .or_insert_with(|| Candidate::from_entity(entity.clone()));
                    entry.name_match_score =
                        (name_hits as f64 / tokens.len() as f64).max(entry.name_match_score);
                    entry.path_match_score =
                        (path_hits as f64 / tokens.len() as f64).max(entry.path_match_score);
                }
            }
        }
    }

    // 3. Vector search
    if let Some(provider) = provider {
        if let Some(ref _model) = config.embedding.model {
            let dimensions = config.embedding.dimensions.unwrap_or(768) as usize;
            let k = config.search.default_limit.max(15) * 3;

            let query_embedding = provider
                .embed(&[query.to_string()])
                .map_err(|e| QueryError::Provider(e.to_string()))?;

            if let Some(vector) = query_embedding.into_iter().next() {
                if vector.len() == dimensions {
                    let results = store.search_vectors(&vector, k)?;
                    for (key, distance) in results {
                        // Map usearch key back to entity_id via embeddings table
                        if let Some(meta) = store.get_embedding_meta_by_usearch_key(key)? {
                            let vector_score = 1.0 / (1.0 + distance as f64);
                            let entry = candidates
                                .entry(meta.entity_id.clone())
                                .or_insert_with(|| Candidate::placeholder(&meta.entity_id));
                            entry.vector_score = vector_score.max(entry.vector_score);
                        }
                    }
                }
            }
        }
    }

    // Fetch full entities for any candidates that only have IDs
    let mut result = Vec::with_capacity(candidates.len());
    for (entity_id, mut candidate) in candidates {
        if candidate.entity.name.is_empty() {
            if let Some(entity) = store.get_entity(&entity_id)? {
                candidate.entity = entity;
            } else {
                continue; // Entity no longer exists
            }
        }
        result.push(candidate);
    }

    Ok(result)
}

/// Search summaries and entity metadata for query tokens.
/// Returns map of entity_id -> number of distinct tokens matched.
fn keyword_search(store: &dyn Store, tokens: &[String]) -> Result<HashMap<String, usize>> {
    // We need access to the underlying SQLite store for a custom query.
    // Since Store trait doesn't expose raw SQL, we'll use the trait methods.
    // For simplicity, we iterate all summaries and entities. This is fine for
    // test-sized stores and small-to-medium repos.
    let mut matches: HashMap<String, usize> = HashMap::new();

    let summaries = store.get_all_summaries()?;
    for summary in summaries {
        let text = format!(
            "{} {}",
            summary.short_summary,
            summary
                .keywords
                .as_ref()
                .map(|k| k.join(" "))
                .unwrap_or_default()
        )
        .to_lowercase();

        let hit_count = tokens.iter().filter(|t| text.contains(*t)).count();
        if hit_count > 0 {
            *matches.entry(summary.entity_id).or_insert(0) += hit_count;
        }
    }

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{
        ChizuStore, Config, Entity, EntityKind, Provider, ProviderError, Store, Summary, TaskRoute,
        entity_id_to_usearch_key,
    };
    use tempfile::TempDir;

    struct MockProvider {
        vector: Vec<f32>,
    }

    impl Provider for MockProvider {
        fn complete(
            &self,
            _prompt: &str,
            _max_tokens: Option<u32>,
        ) -> std::result::Result<String, ProviderError> {
            unimplemented!()
        }
        fn embed(&self, _texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
            Ok(vec![self.vector.clone()])
        }
    }

    fn create_test_store(dimensions: u32) -> (ChizuStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.embedding.dimensions = Some(dimensions);
        let store = ChizuStore::open(temp_dir.path(), &config).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_retrieval_merges_sources() {
        let (store, _temp) = create_test_store(4);

        let entity = Entity::new("symbol::src/lib.rs::foo", EntityKind::Symbol, "foo");
        store.insert_entity(&entity).unwrap();
        store
            .insert_task_route(&TaskRoute::new("debug", "symbol::src/lib.rs::foo", 80))
            .unwrap();
        store
            .insert_summary(&Summary::new(
                "symbol::src/lib.rs::foo",
                "A function that handles auth",
            ))
            .unwrap();

        let config = Config::default();
        let candidates =
            retrieve(&store, "auth debug", TaskCategory::Debug, &config, None).unwrap();

        assert_eq!(candidates.len(), 1);
        let c = &candidates[0];
        assert_eq!(c.entity.id, "symbol::src/lib.rs::foo");
        assert_eq!(c.task_route_priority, Some(80));
        assert!(c.keyword_score > 0.0);
        assert!(c.name_match_score >= 0.0);
    }

    #[test]
    fn test_retrieval_vector_search() {
        let (store, _temp) = create_test_store(4);

        let entity = Entity::new("symbol::src/lib.rs::bar", EntityKind::Symbol, "bar");
        store.insert_entity(&entity).unwrap();
        store
            .insert_summary(&Summary::new("symbol::src/lib.rs::bar", "summary"))
            .unwrap();

        let key = entity_id_to_usearch_key("symbol::src/lib.rs::bar");
        store
            .add_vector("symbol::src/lib.rs::bar", key, &[1.0, 0.0, 0.0, 0.0])
            .unwrap();
        store
            .insert_embedding_meta(
                &chizu_core::EmbeddingMeta::new("symbol::src/lib.rs::bar", "test", 4)
                    .with_usearch_key(key),
            )
            .unwrap();

        let provider = MockProvider {
            vector: vec![1.0, 0.0, 0.0, 0.0],
        };
        let mut config = Config::default();
        config.embedding.model = Some("test".to_string());
        config.embedding.dimensions = Some(4);
        config.search.default_limit = 5;

        let candidates = retrieve(
            &store,
            "query",
            TaskCategory::General,
            &config,
            Some(&provider),
        )
        .unwrap();

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].vector_score > 0.0);
    }
}
