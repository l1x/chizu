use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use chizu_core::{ChizuStore, Entity, Provider, Store, Summary, SummaryConfig};
use tracing::{debug, error, info};

use crate::error::Result;

/// Statistics from a summary generation run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SummaryStats {
    pub generated: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// A work item prepared for LLM summarization.
struct SummaryWork {
    entity_id: String,
    prompt: String,
    source_hash: String,
}

/// Generates summaries for entities using an LLM provider.
pub struct Summarizer<'a> {
    provider: &'a dyn Provider,
    config: &'a SummaryConfig,
}

impl<'a> Summarizer<'a> {
    pub fn new(provider: &'a dyn Provider, config: &'a SummaryConfig) -> Self {
        Self { provider, config }
    }

    pub fn run(&self, store: &ChizuStore, repo_root: &Path) -> Result<SummaryStats> {
        let mut stats = SummaryStats::default();
        let entities = store.get_entities_by_kind(chizu_core::EntityKind::Symbol)?;

        if entities.is_empty() {
            debug!("No symbols to summarize");
            return Ok(stats);
        }

        // Phase 1: collect work items (reads files, checks cache — single-threaded)
        let mut file_cache: HashMap<String, String> = HashMap::new();
        let mut work_items: Vec<SummaryWork> = Vec::new();

        for entity in &entities {
            let Some(ref path) = entity.path else {
                stats.skipped += 1;
                continue;
            };

            let snippet = match extract_snippet(repo_root, path, entity.line_start, entity.line_end, &mut file_cache) {
                Some(s) => s,
                None => {
                    debug!("No snippet for entity {} at {} — skipping", entity.id, path);
                    stats.skipped += 1;
                    continue;
                }
            };

            let source_hash = blake3::hash(snippet.as_bytes()).to_string();

            if let Some(existing) = store.get_summary(&entity.id)? {
                if existing.source_hash.as_ref() == Some(&source_hash) {
                    debug!("Summary for {} is up to date", entity.id);
                    stats.skipped += 1;
                    continue;
                }
            }

            let prompt = build_prompt(entity, &snippet);
            work_items.push(SummaryWork {
                entity_id: entity.id.clone(),
                prompt,
                source_hash,
            });
        }

        if work_items.is_empty() {
            return Ok(stats);
        }

        let concurrency = self.config.concurrency.unwrap_or(4).max(1);
        info!("  {} symbols to summarize (concurrency={})", work_items.len(), concurrency);

        // Phase 2: call LLM in parallel
        let work_iter = Mutex::new(work_items.iter());
        let max_tokens = self.config.max_tokens;

        std::thread::scope(|s| {
            let handles: Vec<_> = (0..concurrency)
                .map(|_| {
                    s.spawn(|| {
                        let mut results = Vec::new();
                        loop {
                            let item = {
                                let mut iter = work_iter.lock().unwrap();
                                iter.next()
                            };
                            let Some(item) = item else { break };

                            info!("  summarizing {}", item.entity_id);
                            let llm_start = Instant::now();
                            let result = self.provider.complete(&item.prompt, max_tokens);
                            let elapsed = llm_start.elapsed().as_secs_f64() * 1000.0;
                            info!("  llm latency: {:.1}ms ({})", elapsed, item.entity_id);

                            results.push((
                                item.entity_id.clone(),
                                item.source_hash.clone(),
                                result,
                            ));
                        }
                        results
                    })
                })
                .collect();

            // Phase 3: collect results and write to store (single-threaded)
            for handle in handles {
                for (entity_id, source_hash, result) in handle.join().unwrap() {
                    match result {
                        Ok(response) => {
                            match parse_summary_response(&entity_id, &response) {
                                Ok(summary) => {
                                    let summary = summary.with_source_hash(source_hash);
                                    if let Err(e) = store.insert_summary(&summary) {
                                        error!("Failed to store summary for {}: {}", entity_id, e);
                                        stats.failed += 1;
                                    } else {
                                        stats.generated += 1;
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse summary for {}: {}", entity_id, e);
                                    stats.failed += 1;
                                }
                            }
                        }
                        Err(e) => {
                            error!("LLM call failed for {}: {}", entity_id, e);
                            stats.failed += 1;
                        }
                    }
                }
            }
        });

        Ok(stats)
    }
}

/// Maximum number of lines to include in a snippet sent to the LLM.
/// Prevents blowing context limits on large functions/structs.
const MAX_SNIPPET_LINES: usize = 200;

fn extract_snippet(
    repo_root: &Path,
    path: &str,
    line_start: Option<u32>,
    line_end: Option<u32>,
    file_cache: &mut HashMap<String, String>,
) -> Option<String> {
    let full_path = repo_root.join(path);

    let content = file_cache.entry(path.to_string()).or_insert_with(|| {
        std::fs::read_to_string(&full_path).unwrap_or_else(|e| {
            debug!("Failed to read {}: {e}", full_path.display());
            String::new()
        })
    });

    if content.is_empty() {
        return None;
    }

    let start = line_start.unwrap_or(1).saturating_sub(1) as usize;
    let end = line_end.unwrap_or(start as u32 + 1).saturating_sub(1) as usize;

    let lines: Vec<&str> = content.lines().collect();
    if start >= lines.len() {
        return None;
    }

    let actual_end = end.min(lines.len() - 1).min(start + MAX_SNIPPET_LINES - 1);
    let snippet = lines[start..=actual_end].join("\n");
    Some(snippet)
}

fn build_prompt(entity: &Entity, snippet: &str) -> String {
    format!(
        r#"You are a code documentation assistant. Given the following code entity, provide a concise summary.

Entity: {}
Kind: {}
File: {}
Lines: {}-{}
Code:
```
{}
```

Respond with ONLY a JSON object in this exact format:
{{
  "short_summary": "one sentence summary",
  "detailed_summary": "2-3 sentence detailed description",
  "keywords": ["keyword1", "keyword2", "keyword3"]
}}"#,
        entity.name,
        entity.kind,
        entity.path.as_deref().unwrap_or("unknown"),
        entity.line_start.unwrap_or(0),
        entity.line_end.unwrap_or(0),
        snippet
    )
}

fn parse_summary_response(entity_id: &str, response: &str) -> Result<Summary> {
    // Try to extract JSON if the response is wrapped in markdown code blocks.
    let json_str = if response.trim().starts_with("```") {
        response
            .lines()
            .skip_while(|l| l.trim().starts_with("```"))
            .take_while(|l| !l.trim().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        response.to_string()
    };

    let mut json_str = json_str.trim().to_string();

    // Recover truncated/malformed JSON from LLMs that cut off or confuse
    // closing delimiters (e.g., `)` instead of `}`).
    if json_str.starts_with('{') && !json_str.ends_with('}') {
        // Replace trailing `)` with `}` (common LLM confusion).
        if json_str.ends_with(')') {
            json_str.pop();
            json_str.push('}');
        }
        // Append missing closing braces.
        let opens = json_str.chars().filter(|&c| c == '{').count();
        let closes = json_str.chars().filter(|&c| c == '}').count();
        for _ in 0..(opens.saturating_sub(closes)) {
            json_str.push('}');
        }
    }

    let json_str = json_str.trim();
    let value: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        crate::error::IndexError::Other(format!(
            "failed to parse summary JSON for {}: {} (raw: {})",
            entity_id, e, response
        ))
    })?;

    let short = value
        .get("short_summary")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            crate::error::IndexError::Other(format!(
                "missing short_summary in response for {}",
                entity_id
            ))
        })?;

    let detailed = value
        .get("detailed_summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let keywords = value
        .get("keywords")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        });

    let mut summary = Summary::new(entity_id, short);
    if let Some(d) = detailed {
        summary = summary.with_detailed(d);
    }
    if let Some(k) = keywords {
        summary = summary.with_keywords(&k.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{ChizuStore, Config, Entity, EntityKind, Provider, ProviderError};
    use std::collections::HashMap;
    use tempfile::TempDir;

    struct MockProvider {
        responses: HashMap<String, String>,
    }

    impl Provider for MockProvider {
        fn complete(&self, prompt: &str, _max_tokens: Option<u32>) -> std::result::Result<String, ProviderError> {
            let key = blake3::hash(prompt.as_bytes()).to_string();
            self.responses
                .get(&key)
                .cloned()
                .or_else(|| {
                    Some(r#"{"short_summary": "default summary", "detailed_summary": "default detailed", "keywords": ["default"]}"#.to_string())
                })
                .ok_or_else(|| ProviderError::Other("no response".into()))
        }

        fn embed(&self, _texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
            unimplemented!()
        }
    }

    fn create_test_store() -> (ChizuStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::default();
        let store = ChizuStore::open(temp_dir.path(), &config).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_parse_summary_response() {
        let response = r#"{"short_summary": "A test function", "detailed_summary": "This function tests things.", "keywords": ["test", "rust"]}"#;
        let summary = parse_summary_response("e1", response).unwrap();
        assert_eq!(summary.short_summary, "A test function");
        assert_eq!(summary.detailed_summary, Some("This function tests things.".to_string()));
        assert_eq!(summary.keywords, Some(vec!["test".to_string(), "rust".to_string()]));
    }

    #[test]
    fn test_parse_summary_with_markdown_wrapping() {
        let response = "```json\n{\"short_summary\": \"wrapped\", \"keywords\": [\"a\"]}\n```";
        let summary = parse_summary_response("e1", response).unwrap();
        assert_eq!(summary.short_summary, "wrapped");
        assert_eq!(summary.keywords, Some(vec!["a".to_string()]));
    }

    #[test]
    fn test_summarizer_generates_and_caches() {
        let (store, temp_dir) = create_test_store();
        let repo_root = temp_dir.path().join("repo");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/lib.rs"), "fn foo() {}\n").unwrap();

        let entity = Entity::new("symbol::src/lib.rs::foo", EntityKind::Symbol, "foo")
            .with_path("src/lib.rs")
            .with_lines(1, 1);
        store.insert_entity(&entity).unwrap();

        let provider = MockProvider {
            responses: HashMap::new(),
        };
        let config = SummaryConfig::default();
        let summarizer = Summarizer::new(&provider, &config);

        let stats1 = summarizer.run(&store, &repo_root).unwrap();
        assert_eq!(stats1.generated, 1);
        assert_eq!(stats1.skipped, 0);

        let summary = store.get_summary("symbol::src/lib.rs::foo").unwrap().unwrap();
        assert_eq!(summary.short_summary, "default summary");
        assert_eq!(summary.detailed_summary, Some("default detailed".to_string()));
        assert_eq!(summary.keywords, Some(vec!["default".to_string()]));
        assert!(summary.source_hash.is_some());

        // Re-run should skip unchanged entity.
        let stats2 = summarizer.run(&store, &repo_root).unwrap();
        assert_eq!(stats2.generated, 0);
        assert_eq!(stats2.skipped, 1);
    }

    #[test]
    fn test_parse_truncated_json_paren_instead_of_brace() {
        // LLM outputs `)` instead of `}` — common confusion.
        let response = r#"{
  "short_summary": "Imports the Path module.",
  "detailed_summary": "Brings in file path functionality.",
  "keywords": ["std", "Path"])"#;
        let summary = parse_summary_response("e1", response).unwrap();
        assert_eq!(summary.short_summary, "Imports the Path module.");
        assert_eq!(summary.keywords, Some(vec!["std".to_string(), "Path".to_string()]));
    }

    #[test]
    fn test_parse_truncated_json_missing_closing_brace() {
        // LLM output cut off after array — missing `}`.
        let response = r#"{
  "short_summary": "A summary.",
  "keywords": ["a", "b"]"#;
        let summary = parse_summary_response("e1", response).unwrap();
        assert_eq!(summary.short_summary, "A summary.");
        assert_eq!(summary.keywords, Some(vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn test_parse_truncated_json_mid_value() {
        // Truncated inside a string value — unrecoverable.
        let response = r#"{"short_summary": "Trunca"#;
        assert!(parse_summary_response("e1", response).is_err());
    }
}
