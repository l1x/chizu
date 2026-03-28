use std::collections::HashMap;

use chizu_core::model::{Entity, VectorSearchResult};
use chizu_core::Store;

/// How a candidate was found.
#[derive(Debug, Clone, PartialEq)]
pub enum RetrievalSource {
    TaskRoute { priority: i64 },
    KeywordMatch,
    NameMatch,
    PathMatch,
    VectorSearch { distance: f32 },
    /// Found via graph expansion (neighbor of a direct match)
    Context { via_entity_id: String },
}

/// A retrieval candidate with its provenance.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub entity: Entity,
    pub sources: Vec<RetrievalSource>,
    pub short_summary: String,
    pub keywords: Vec<String>,
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "shall", "should", "may", "might", "must", "can",
    "could", "of", "in", "to", "for", "with", "on", "at", "from", "by", "about", "as", "into",
    "through", "during", "before", "after", "and", "but", "or", "nor", "not", "so", "yet", "both",
    "either", "neither", "each", "every", "all", "any", "few", "more", "most", "other", "some",
    "such", "no", "only", "own", "same", "than", "too", "very", "just", "it", "its", "this",
    "that", "these", "those", "i", "me", "my", "we", "our", "you", "your", "he", "him", "his",
    "she", "her", "they", "them", "their", "which", "who", "whom", "where", "when",
];

/// Tokenize and remove stopwords from query text.
pub fn tokenize_query(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty() && w.len() > 1 && !STOPWORDS.contains(w))
        .map(String::from)
        .collect()
}

/// Retrieve candidates from multiple sources, merged by entity_id.
pub fn retrieve(
    store: &Store,
    route_names: &[&str],
    query_tokens: &[String],
    query_embedding: Option<&[f32]>,
    vector_k: usize,
) -> chizu_core::Result<HashMap<String, Candidate>> {
    let mut candidates: HashMap<String, Candidate> = HashMap::new();

    // 1. Task route prefilter
    for route_name in route_names {
        let routes = store.routes_for_task(route_name)?;
        for route in routes {
            if let Ok(entity) = store.get_entity(&route.entity_id) {
                let summary = load_summary(store, &entity.id);
                let entry = candidates
                    .entry(entity.id.clone())
                    .or_insert_with(|| Candidate {
                        entity,
                        sources: Vec::new(),
                        short_summary: summary.0,
                        keywords: summary.1,
                    });
                entry.sources.push(RetrievalSource::TaskRoute {
                    priority: route.priority,
                });
            }
        }
    }

    // 2. Keyword / name / path matching over all entities
    if !query_tokens.is_empty() {
        let entities = store.list_entities()?;
        for entity in entities {
            let name_lower = entity.name.to_lowercase();
            let path_lower = entity.path.as_deref().unwrap_or("").to_lowercase();

            let name_match = query_tokens.iter().any(|t| name_lower.contains(t.as_str()));
            let path_match = query_tokens.iter().any(|t| path_lower.contains(t.as_str()));

            let summary = if name_match || path_match {
                load_summary(store, &entity.id)
            } else {
                (String::new(), Vec::new())
            };

            let keyword_match = if !summary.1.is_empty() {
                let kw_lower: Vec<String> = summary.1.iter().map(|k| k.to_lowercase()).collect();
                query_tokens
                    .iter()
                    .any(|t| kw_lower.iter().any(|k| k.contains(t.as_str())))
            } else if !name_match && !path_match {
                // Load summary to check keywords even if name/path didn't match
                let s = load_summary(store, &entity.id);
                if !s.1.is_empty() {
                    let kw_lower: Vec<String> = s.1.iter().map(|k| k.to_lowercase()).collect();
                    let matches = query_tokens
                        .iter()
                        .any(|t| kw_lower.iter().any(|k| k.contains(t.as_str())));
                    if matches {
                        let entry =
                            candidates
                                .entry(entity.id.clone())
                                .or_insert_with(|| Candidate {
                                    entity,
                                    sources: Vec::new(),
                                    short_summary: s.0,
                                    keywords: s.1,
                                });
                        entry.sources.push(RetrievalSource::KeywordMatch);
                    }
                    continue;
                }
                false
            } else {
                false
            };

            if name_match || path_match || keyword_match {
                let entry = candidates
                    .entry(entity.id.clone())
                    .or_insert_with(|| Candidate {
                        entity,
                        sources: Vec::new(),
                        short_summary: summary.0,
                        keywords: summary.1,
                    });
                if name_match {
                    entry.sources.push(RetrievalSource::NameMatch);
                }
                if path_match {
                    entry.sources.push(RetrievalSource::PathMatch);
                }
                if keyword_match {
                    entry.sources.push(RetrievalSource::KeywordMatch);
                }
            }
        }
    }

    // 3. Optional vector search
    if let Some(embedding) = query_embedding {
        let results: Vec<VectorSearchResult> = store.vector_search(embedding, vector_k)?;
        for vsr in results {
            if let Ok(entity) = store.get_entity(&vsr.entity_id) {
                let summary = load_summary(store, &entity.id);
                let entry = candidates
                    .entry(entity.id.clone())
                    .or_insert_with(|| Candidate {
                        entity,
                        sources: Vec::new(),
                        short_summary: summary.0,
                        keywords: summary.1,
                    });
                entry.sources.push(RetrievalSource::VectorSearch {
                    distance: vsr.distance,
                });
            }
        }
    }

    Ok(candidates)
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
    use chizu_core::model::{Entity, EntityKind, Summary, TaskRoute};
    use chizu_core::Store;

    fn make_entity(id: &str, name: &str, kind: EntityKind, path: Option<&str>) -> Entity {
        Entity {
            id: id.to_string(),
            kind,
            name: name.to_string(),
            component_id: None,
            path: path.map(String::from),
            language: Some("rust".to_string()),
            line_start: Some(1),
            line_end: Some(50),
            visibility: Some("pub".to_string()),
            exported: true,
        }
    }

    fn make_summary(entity_id: &str, short: &str, keywords: &[&str]) -> Summary {
        Summary {
            entity_id: entity_id.to_string(),
            short_summary: short.to_string(),
            detailed_summary: None,
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            source_hash: None,
        }
    }

    /// Build a store with a known set of entities, summaries, and routes.
    fn build_test_store() -> Store {
        let store = Store::open_in_memory().unwrap();

        // Entities
        let engine = make_entity(
            "symbol::Engine",
            "Engine",
            EntityKind::Symbol,
            Some("src/engine.rs"),
        );
        let parser = make_entity(
            "symbol::Parser",
            "Parser",
            EntityKind::Symbol,
            Some("src/parser.rs"),
        );
        let store_entity = make_entity(
            "component::store",
            "store",
            EntityKind::Component,
            Some("src/store/mod.rs"),
        );
        let config = make_entity(
            "symbol::Config",
            "Config",
            EntityKind::Symbol,
            Some("src/config.rs"),
        );
        let readme = make_entity("doc::README", "README", EntityKind::Doc, Some("README.md"));

        for e in [&engine, &parser, &store_entity, &config, &readme] {
            store.insert_entity(e).unwrap();
        }

        // Summaries
        store
            .upsert_summary(&make_summary(
                "symbol::Engine",
                "Core execution engine",
                &["engine", "execution", "core"],
            ))
            .unwrap();
        store
            .upsert_summary(&make_summary(
                "symbol::Parser",
                "Parses source code into AST",
                &["parser", "ast", "syntax"],
            ))
            .unwrap();
        store
            .upsert_summary(&make_summary(
                "component::store",
                "Persistence layer",
                &["store", "database", "persistence"],
            ))
            .unwrap();
        store
            .upsert_summary(&make_summary(
                "symbol::Config",
                "Application configuration",
                &["config", "settings", "environment"],
            ))
            .unwrap();

        // Task routes
        store
            .insert_task_route(&TaskRoute {
                task_name: "build".to_string(),
                entity_id: "symbol::Engine".to_string(),
                priority: 80,
            })
            .unwrap();
        store
            .insert_task_route(&TaskRoute {
                task_name: "build".to_string(),
                entity_id: "component::store".to_string(),
                priority: 60,
            })
            .unwrap();
        store
            .insert_task_route(&TaskRoute {
                task_name: "debug".to_string(),
                entity_id: "symbol::Parser".to_string(),
                priority: 90,
            })
            .unwrap();

        store
    }

    // --- tokenize_query() ---

    #[test]
    fn tokenize_removes_stopwords() {
        let tokens = tokenize_query("does the engine work for them");
        assert!(!tokens.contains(&"does".to_string()), "does is a stopword");
        assert!(!tokens.contains(&"the".to_string()), "the is a stopword");
        assert!(!tokens.contains(&"for".to_string()), "for is a stopword");
        assert!(!tokens.contains(&"them".to_string()), "them is a stopword");
        assert!(tokens.contains(&"engine".to_string()));
        assert!(tokens.contains(&"work".to_string()));
    }

    #[test]
    fn tokenize_lowercases() {
        let tokens = tokenize_query("Engine Parser");
        assert!(tokens.contains(&"engine".to_string()));
        assert!(tokens.contains(&"parser".to_string()));
    }

    #[test]
    fn tokenize_removes_short_tokens() {
        let tokens = tokenize_query("a I x go");
        // "a" (len 1), "I" -> "i" (len 1), "x" (len 1) filtered; "go" (len 2) kept
        assert!(!tokens.contains(&"a".to_string()));
        assert!(!tokens.contains(&"x".to_string()));
        assert!(tokens.contains(&"go".to_string()));
    }

    #[test]
    fn tokenize_splits_on_punctuation() {
        let tokens = tokenize_query("engine.parse()");
        assert!(tokens.contains(&"engine".to_string()));
        assert!(tokens.contains(&"parse".to_string()));
    }

    #[test]
    fn tokenize_preserves_underscores() {
        let tokens = tokenize_query("task_route store_backend");
        assert!(tokens.contains(&"task_route".to_string()));
        assert!(tokens.contains(&"store_backend".to_string()));
    }

    #[test]
    fn tokenize_empty_input() {
        assert!(tokenize_query("").is_empty());
    }

    #[test]
    fn tokenize_all_stopwords() {
        let tokens = tokenize_query("the is a an");
        assert!(tokens.is_empty());
    }

    // --- retrieve() with task routes ---

    #[test]
    fn retrieve_finds_task_route_candidates() {
        let store = build_test_store();
        let tokens = tokenize_query("something unrelated");
        let candidates = retrieve(&store, &["build"], &tokens, None, 10).unwrap();

        // "build" route has Engine (pri 80) and store (pri 60)
        assert!(candidates.contains_key("symbol::Engine"));
        assert!(candidates.contains_key("component::store"));

        let engine = &candidates["symbol::Engine"];
        assert!(engine
            .sources
            .iter()
            .any(|s| matches!(s, RetrievalSource::TaskRoute { priority: 80 })));
    }

    #[test]
    fn retrieve_task_route_for_nonexistent_task_returns_nothing_extra() {
        let store = build_test_store();
        let tokens: Vec<String> = vec![];
        let candidates = retrieve(&store, &["nonexistent"], &tokens, None, 10).unwrap();
        assert!(candidates.is_empty());
    }

    // --- retrieve() with name matching ---

    #[test]
    fn retrieve_name_match() {
        let store = build_test_store();
        let tokens = tokenize_query("Engine");
        let candidates = retrieve(&store, &[], &tokens, None, 10).unwrap();

        assert!(candidates.contains_key("symbol::Engine"));
        let engine = &candidates["symbol::Engine"];
        assert!(engine
            .sources
            .iter()
            .any(|s| matches!(s, RetrievalSource::NameMatch)));
    }

    // --- retrieve() with path matching ---

    #[test]
    fn retrieve_path_match() {
        let store = build_test_store();
        let tokens = tokenize_query("config");
        let candidates = retrieve(&store, &[], &tokens, None, 10).unwrap();

        assert!(candidates.contains_key("symbol::Config"));
        let config = &candidates["symbol::Config"];
        // "config" matches both name ("Config") and path ("src/config.rs")
        assert!(config
            .sources
            .iter()
            .any(|s| matches!(s, RetrievalSource::PathMatch)));
    }

    // --- retrieve() with keyword matching ---

    #[test]
    fn retrieve_keyword_match_from_summary() {
        let store = build_test_store();
        // "persistence" is a keyword on component::store, but not in its name or path
        let tokens = tokenize_query("persistence");
        let candidates = retrieve(&store, &[], &tokens, None, 10).unwrap();

        // component::store has keyword "persistence" but name "store" and path "src/store/mod.rs"
        // "persistence" doesn't match "store" name, but keyword check should find it
        // Actually "persistence" doesn't match path "src/store/mod.rs" either
        // The code checks: if !name_match && !path_match, load summary and check keywords
        assert!(
            candidates.contains_key("component::store"),
            "should find store via keyword 'persistence'"
        );
        let store_cand = &candidates["component::store"];
        assert!(store_cand
            .sources
            .iter()
            .any(|s| matches!(s, RetrievalSource::KeywordMatch)));
    }

    // --- retrieve() with multiple sources ---

    #[test]
    fn retrieve_merges_multiple_sources() {
        let store = build_test_store();
        // "engine" matches name, path, and keywords of symbol::Engine
        // "build" route also includes symbol::Engine
        let tokens = tokenize_query("engine");
        let candidates = retrieve(&store, &["build"], &tokens, None, 10).unwrap();

        let engine = &candidates["symbol::Engine"];
        let has_route = engine
            .sources
            .iter()
            .any(|s| matches!(s, RetrievalSource::TaskRoute { .. }));
        let has_name = engine
            .sources
            .iter()
            .any(|s| matches!(s, RetrievalSource::NameMatch));
        assert!(has_route, "should have task route source");
        assert!(has_name, "should have name match source");
    }

    // --- retrieve() with empty tokens ---

    #[test]
    fn retrieve_empty_tokens_no_keyword_matching() {
        let store = build_test_store();
        // With no tokens and no routes, should get no candidates
        let candidates = retrieve(&store, &[], &[], None, 10).unwrap();
        assert!(candidates.is_empty());
    }

    // --- retrieve() loads summaries ---

    #[test]
    fn retrieve_populates_summary_and_keywords() {
        let store = build_test_store();
        let tokens = tokenize_query("Engine");
        let candidates = retrieve(&store, &[], &tokens, None, 10).unwrap();

        let engine = &candidates["symbol::Engine"];
        assert_eq!(engine.short_summary, "Core execution engine");
        assert!(engine.keywords.contains(&"engine".to_string()));
    }

    #[test]
    fn retrieve_entity_without_summary_gets_empty() {
        let store = build_test_store();
        let tokens = tokenize_query("README");
        let candidates = retrieve(&store, &[], &tokens, None, 10).unwrap();

        let readme = &candidates["doc::README"];
        assert_eq!(readme.short_summary, "");
        assert!(readme.keywords.is_empty());
    }

    // --- retrieve() with multiple route names ---

    #[test]
    fn retrieve_multiple_route_names() {
        let store = build_test_store();
        let tokens: Vec<String> = vec![];
        let candidates = retrieve(&store, &["build", "debug"], &tokens, None, 10).unwrap();

        // build: Engine, store; debug: Parser
        assert!(candidates.contains_key("symbol::Engine"));
        assert!(candidates.contains_key("component::store"));
        assert!(candidates.contains_key("symbol::Parser"));
    }
}
