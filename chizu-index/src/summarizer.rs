use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::io::Read;
use std::ops::Range;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chizu_core::{ChizuStore, Entity, Provider, ProviderError, Store, Summary, SummaryConfig};
use rustc_hash::FxHasher;
use tiktoken_rs::{CoreBPE, Rank};
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
#[derive(Clone)]
struct SummaryWork {
    entity_id: String,
    prompt_input: String,
    source_hash: String,
}

struct BatchOutcome {
    start: usize,
    end: usize,
    result: std::result::Result<String, ProviderError>,
}

enum BatchPlanner {
    Exact(ExactBatchPlanner),
    Heuristic,
}

struct ExactBatchPlanner {
    tokenizer: &'static CoreBPE,
    context_window: usize,
    safety_margin_tokens: usize,
}

impl ExactBatchPlanner {
    const LLAMA3_8B_CONTEXT_WINDOW: usize = 8192;
    const DEFAULT_SAFETY_MARGIN_TOKENS: usize = 256;
    const DEFAULT_OUTPUT_TOKENS_PER_ITEM: u32 = 512;

    fn for_summary_model(model: Option<&str>) -> Option<Self> {
        let model = model?;
        if !is_llama3_8b_model(model) {
            return None;
        }

        Some(Self {
            tokenizer: llama3_tokenizer(),
            context_window: Self::LLAMA3_8B_CONTEXT_WINDOW,
            safety_margin_tokens: Self::DEFAULT_SAFETY_MARGIN_TOKENS,
        })
    }

    fn plan_batches(
        &self,
        work_items: &[SummaryWork],
        max_batch_size: usize,
        max_tokens: Option<u32>,
    ) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let mut start = 0;

        while start < work_items.len() {
            let mut end = start;
            let mut best_end = start;

            while end < work_items.len() && end - start < max_batch_size {
                let candidate_end = end + 1;
                if self.batch_fits(&work_items[start..candidate_end], max_tokens) {
                    best_end = candidate_end;
                    end = candidate_end;
                    continue;
                }

                break;
            }

            // Always make progress, even if a single item exceeds the safe budget.
            let next_end = if best_end == start {
                start + 1
            } else {
                best_end
            };
            ranges.push(start..next_end);
            start = next_end;
        }

        ranges
    }

    fn batch_fits(&self, batch: &[SummaryWork], max_tokens: Option<u32>) -> bool {
        let prompt = if batch.len() == 1 {
            build_single_prompt(&batch[0].prompt_input)
        } else {
            build_batch_prompt(batch)
        };
        let prompt_tokens = count_llama3_chat_tokens(self.tokenizer, &prompt);
        let reserved_output_tokens = scale_max_tokens(
            max_tokens.or(Some(Self::DEFAULT_OUTPUT_TOKENS_PER_ITEM)),
            batch.len(),
        )
        .unwrap_or(Self::DEFAULT_OUTPUT_TOKENS_PER_ITEM)
            as usize;

        prompt_tokens + reserved_output_tokens + self.safety_margin_tokens <= self.context_window
    }
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
        let all_entities = store.get_entities_by_kind(chizu_core::EntityKind::Symbol)?;
        let exported_only = self.config.exported_only.unwrap_or(true);
        let entities: Vec<_> = if exported_only {
            all_entities.into_iter().filter(|e| e.exported).collect()
        } else {
            all_entities
        };

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

            let snippet = match extract_snippet(
                repo_root,
                path,
                entity.line_start,
                entity.line_end,
                &mut file_cache,
            ) {
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

            let prompt_input = build_prompt_input(entity, &snippet);
            work_items.push(SummaryWork {
                entity_id: entity.id.clone(),
                prompt_input,
                source_hash,
            });
        }

        if work_items.is_empty() {
            return Ok(stats);
        }

        let batch_size = self.config.batch_size.unwrap_or(4).max(1);
        let concurrency = self.config.concurrency.unwrap_or(1).max(1);
        let batch_planner = match ExactBatchPlanner::for_summary_model(self.config.model.as_deref())
        {
            Some(planner) => {
                info!(
                    "  using exact token-aware batching for {}",
                    self.config.model.as_deref().unwrap_or("unknown")
                );
                BatchPlanner::Exact(planner)
            }
            None => BatchPlanner::Heuristic,
        };
        let batch_ranges = match &batch_planner {
            BatchPlanner::Exact(planner) => {
                planner.plan_batches(&work_items, batch_size, self.config.max_tokens)
            }
            BatchPlanner::Heuristic => chunk_ranges(work_items.len(), batch_size),
        };
        let exact_tokenizer = match &batch_planner {
            BatchPlanner::Exact(planner) => Some(planner.tokenizer),
            BatchPlanner::Heuristic => None,
        };
        info!(
            "  {} symbols to summarize ({} batches, batch_size<= {}, concurrency={})",
            work_items.len(),
            batch_ranges.len(),
            batch_size,
            concurrency
        );

        // Phase 2: call LLM in parallel
        let next_batch = Mutex::new(0usize);

        std::thread::scope(|s| {
            let handles: Vec<_> = (0..concurrency)
                .map(|_| {
                    s.spawn(|| {
                        let mut results = Vec::new();
                        loop {
                            let batch_index = {
                                let mut next = next_batch.lock().unwrap();
                                if *next >= batch_ranges.len() {
                                    break;
                                }
                                let batch_index = *next;
                                *next += 1;
                                batch_index
                            };
                            let range = batch_ranges[batch_index].clone();
                            let start = range.start;
                            let end = range.end;
                            let batch = &work_items[start..end];
                            let prompt = if batch.len() == 1 {
                                build_single_prompt(&batch[0].prompt_input)
                            } else {
                                build_batch_prompt(batch)
                            };
                            let max_tokens = scale_max_tokens(self.config.max_tokens, batch.len());
                            let prompt_tokens =
                                exact_tokenizer.map(|tokenizer| count_llama3_chat_tokens(tokenizer, &prompt));

                            info!(
                                "  summarizing batch of {} symbols (prompt_tokens={:?}, requested_max_tokens={:?})",
                                batch.len(),
                                prompt_tokens,
                                max_tokens,
                            );
                            let llm_start = Instant::now();
                            let result = self.provider.complete(&prompt, max_tokens);
                            let elapsed = llm_start.elapsed().as_secs_f64() * 1000.0;
                            let response_tokens = match &result {
                                Ok(response) => exact_tokenizer
                                    .map(|tokenizer| count_llama3_text_tokens(tokenizer, response)),
                                Err(_) => None,
                            };
                            info!(
                                "  llm latency: {:.1}ms ({} symbols, prompt_tokens={:?}, response_tokens={:?}, requested_max_tokens={:?})",
                                elapsed,
                                batch.len(),
                                prompt_tokens,
                                response_tokens,
                                max_tokens,
                            );

                            results.push(BatchOutcome { start, end, result });
                        }
                        results
                    })
                })
                .collect();

            // Phase 3: collect results and write to store (single-threaded)
            for handle in handles {
                for outcome in handle.join().unwrap() {
                    let batch = &work_items[outcome.start..outcome.end];
                    self.store_batch_result(store, batch, outcome.result, &mut stats);
                }
            }
        });

        Ok(stats)
    }

    fn store_batch_result(
        &self,
        store: &ChizuStore,
        batch: &[SummaryWork],
        result: std::result::Result<String, ProviderError>,
        stats: &mut SummaryStats,
    ) {
        if batch.len() == 1 {
            self.store_single_result(store, &batch[0], result, stats);
            return;
        }

        match result {
            Ok(response) => match parse_batch_summary_response(&response) {
                Ok(mut summaries) => {
                    let missing: Vec<_> = batch
                        .iter()
                        .filter(|item| !summaries.contains_key(&item.entity_id))
                        .map(|item| item.entity_id.clone())
                        .collect();

                    if !missing.is_empty() {
                        error!(
                            "Batched summary response omitted {} entities; falling back to singles",
                            missing.join(", ")
                        );
                        self.fallback_to_singles(store, batch, stats);
                        return;
                    }

                    for item in batch {
                        let Some(summary) = summaries.remove(&item.entity_id) else {
                            error!(
                                "Batched summary response lost {} during processing; falling back to singles",
                                item.entity_id
                            );
                            self.fallback_to_singles(store, batch, stats);
                            return;
                        };
                        self.insert_summary(store, item, summary, stats);
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to parse batched summary response: {e}; falling back to singles"
                    );
                    self.fallback_to_singles(store, batch, stats);
                }
            },
            Err(e) => {
                error!("Batched LLM call failed: {e}; falling back to singles");
                self.fallback_to_singles(store, batch, stats);
            }
        }
    }

    fn fallback_to_singles(
        &self,
        store: &ChizuStore,
        batch: &[SummaryWork],
        stats: &mut SummaryStats,
    ) {
        for item in batch {
            self.run_single_request(store, item, stats);
        }
    }

    fn run_single_request(&self, store: &ChizuStore, item: &SummaryWork, stats: &mut SummaryStats) {
        let prompt = build_single_prompt(&item.prompt_input);
        let prompt_tokens = exact_tokenizer_for_model(self.config.model.as_deref())
            .map(|tokenizer| count_llama3_chat_tokens(tokenizer, &prompt));
        info!(
            "  summarizing {} (prompt_tokens={:?}, requested_max_tokens={:?})",
            item.entity_id,
            prompt_tokens,
            self.config.max_tokens,
        );
        let llm_start = Instant::now();
        let result = self.provider.complete(&prompt, self.config.max_tokens);
        let elapsed = llm_start.elapsed().as_secs_f64() * 1000.0;
        let response_tokens = match &result {
            Ok(response) => exact_tokenizer_for_model(self.config.model.as_deref())
                .map(|tokenizer| count_llama3_text_tokens(tokenizer, response)),
            Err(_) => None,
        };
        info!(
            "  llm latency (single): {:.1}ms ({}, prompt_tokens={:?}, response_tokens={:?}, requested_max_tokens={:?})",
            elapsed, item.entity_id
            , prompt_tokens
            , response_tokens
            , self.config.max_tokens
        );
        self.store_single_result(store, item, result, stats);
    }

    fn store_single_result(
        &self,
        store: &ChizuStore,
        item: &SummaryWork,
        result: std::result::Result<String, ProviderError>,
        stats: &mut SummaryStats,
    ) {
        match result {
            Ok(response) => match parse_summary_response(&item.entity_id, &response) {
                Ok(summary) => self.insert_summary(store, item, summary, stats),
                Err(e) => {
                    error!("Failed to parse summary for {}: {}", item.entity_id, e);
                    stats.failed += 1;
                }
            },
            Err(e) => {
                error!("LLM call failed for {}: {}", item.entity_id, e);
                stats.failed += 1;
            }
        }
    }

    fn insert_summary(
        &self,
        store: &ChizuStore,
        item: &SummaryWork,
        summary: Summary,
        stats: &mut SummaryStats,
    ) {
        let summary = summary.with_source_hash(item.source_hash.clone());
        if let Err(e) = store.insert_summary(&summary) {
            error!("Failed to store summary for {}: {}", item.entity_id, e);
            stats.failed += 1;
        } else {
            stats.generated += 1;
        }
    }
}

const LLAMA3_PATTERN: &str = "(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\\r\\n\\p{L}\\p{N}]?\\p{L}+|\\p{N}{1,3}| ?[^\\s\\p{L}\\p{N}]+[\\r\\n]*|\\s*[\\r\\n]+|\\s+";

const LLAMA3_TIKTOKEN_DATA: &[u8] = include_bytes!("../assets/llama3.tiktoken.zst");

type RankMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;

fn llama3_tokenizer() -> &'static CoreBPE {
    static TOKENIZER: OnceLock<CoreBPE> = OnceLock::new();
    TOKENIZER.get_or_init(build_llama3_tokenizer)
}

fn build_llama3_tokenizer() -> CoreBPE {
    let encoder = parse_llama3_encoder(LLAMA3_TIKTOKEN_DATA);
    let mut special_tokens: RankMap<String, Rank> = Default::default();
    special_tokens.insert("<|begin_of_text|>".to_string(), 128000);
    special_tokens.insert("<|end_of_text|>".to_string(), 128001);
    special_tokens.insert("<|finetune_right_pad_id|>".to_string(), 128004);
    special_tokens.insert("<|start_header_id|>".to_string(), 128006);
    special_tokens.insert("<|end_header_id|>".to_string(), 128007);
    special_tokens.insert("<|eom_id|>".to_string(), 128008);
    special_tokens.insert("<|eot_id|>".to_string(), 128009);
    special_tokens.insert("<|python_tag|>".to_string(), 128010);

    CoreBPE::new(encoder, special_tokens, LLAMA3_PATTERN)
        .expect("llama3 tokenizer asset must be valid")
}

fn parse_llama3_encoder(compressed: &[u8]) -> RankMap<Vec<u8>, Rank> {
    let mut decoder =
        ruzstd::decoding::StreamingDecoder::new(compressed).expect("zstd decompression failed");
    let mut data = Vec::new();
    decoder
        .read_to_end(&mut data)
        .expect("zstd decompression failed");

    let text = std::str::from_utf8(&data).expect("llama3 tokenizer data must be valid UTF-8");
    let mut encoder: RankMap<Vec<u8>, Rank> = Default::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let token = STANDARD
            .decode(parts.next().expect("missing token"))
            .expect("invalid base64 token");
        let rank: Rank = parts
            .next()
            .expect("missing rank")
            .parse()
            .expect("invalid rank");
        encoder.insert(token, rank);
    }
    encoder
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

fn build_prompt_input(entity: &Entity, snippet: &str) -> String {
    format!(
        r#"Entity ID: {}
Entity: {}
Kind: {}
File: {}
Lines: {}-{}
Code:
```
{}
```"#,
        entity.id,
        entity.name,
        entity.kind,
        entity.path.as_deref().unwrap_or("unknown"),
        entity.line_start.unwrap_or(0),
        entity.line_end.unwrap_or(0),
        snippet
    )
}

fn build_single_prompt(prompt_input: &str) -> String {
    format!(
        r#"You are a code documentation assistant. Given the following code entity, provide a concise summary.

{}

Respond with ONLY a JSON object in this exact format:
{{
  "short_summary": "one sentence summary",
  "detailed_summary": "2-3 sentence detailed description",
  "keywords": ["keyword1", "keyword2", "keyword3"]
}}"#,
        prompt_input
    )
}

fn build_batch_prompt(batch: &[SummaryWork]) -> String {
    let entities = batch
        .iter()
        .enumerate()
        .map(|(index, item)| format!("Entity {}:\n{}", index + 1, item.prompt_input))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r#"You are a code documentation assistant. Given the following code entities, provide a concise summary for each one.

{}

Respond with ONLY a JSON object in this exact format:
{{
  "summaries": [
    {{
      "entity_id": "symbol::src/lib.rs::foo",
      "short_summary": "one sentence summary",
      "detailed_summary": "2-3 sentence detailed description",
      "keywords": ["keyword1", "keyword2", "keyword3"]
    }}
  ]
}}

Rules:
- Return exactly one summary object for every entity above.
- Preserve each `entity_id` exactly.
- Do not omit entities.
- Do not wrap the response in markdown."#,
        entities
    )
}

fn scale_max_tokens(max_tokens: Option<u32>, batch_len: usize) -> Option<u32> {
    max_tokens.map(|tokens| tokens.saturating_mul(batch_len.max(1) as u32))
}

fn chunk_ranges(len: usize, chunk_size: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < len {
        let end = (start + chunk_size).min(len);
        ranges.push(start..end);
        start = end;
    }
    ranges
}

fn is_llama3_8b_model(model: &str) -> bool {
    model.trim() == "llama3:8b"
}

fn exact_tokenizer_for_model(model: Option<&str>) -> Option<&'static CoreBPE> {
    let model = model?;
    if is_llama3_8b_model(model) {
        Some(llama3_tokenizer())
    } else {
        None
    }
}

fn count_llama3_chat_tokens(tokenizer: &CoreBPE, user_prompt: &str) -> usize {
    let rendered = format!(
        "<|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
        user_prompt
    );
    tokenizer.encode_with_special_tokens(&rendered).len()
}

fn count_llama3_text_tokens(tokenizer: &CoreBPE, text: &str) -> usize {
    tokenizer.encode_with_special_tokens(text).len()
}

fn parse_response_json(response: &str) -> Result<serde_json::Value> {
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
    serde_json::from_str(json_str).map_err(|e| {
        crate::error::IndexError::Other(format!(
            "failed to parse summary JSON for {}: {} (raw: {})",
            "<response>", e, response
        ))
    })
}

fn parse_summary_value(entity_id: &str, value: &serde_json::Value) -> Result<Summary> {
    let object = value.as_object().ok_or_else(|| {
        crate::error::IndexError::Other(format!(
            "summary payload for {} is not a JSON object",
            entity_id
        ))
    })?;

    let short = object
        .get("short_summary")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            crate::error::IndexError::Other(format!(
                "missing short_summary in response for {}",
                entity_id
            ))
        })?;

    let detailed = object
        .get("detailed_summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let keywords = object
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

fn parse_summary_response(entity_id: &str, response: &str) -> Result<Summary> {
    let value = parse_response_json(response)?;
    parse_summary_value(entity_id, &value)
}

fn parse_batch_summary_response(response: &str) -> Result<HashMap<String, Summary>> {
    let value = parse_response_json(response)?;

    if let Some(items) = value.get("summaries").and_then(|v| v.as_array()) {
        let mut summaries = HashMap::with_capacity(items.len());
        for item in items {
            let entity_id = item
                .get("entity_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    crate::error::IndexError::Other(
                        "missing entity_id in batched summary response".into(),
                    )
                })?;
            let summary = parse_summary_value(entity_id, item)?;
            if summaries.insert(entity_id.to_string(), summary).is_some() {
                return Err(crate::error::IndexError::Other(format!(
                    "duplicate entity_id in batched summary response: {}",
                    entity_id
                )));
            }
        }
        return Ok(summaries);
    }

    let object = value.as_object().ok_or_else(|| {
        crate::error::IndexError::Other("batched summary response is not a JSON object".into())
    })?;

    let mut summaries = HashMap::with_capacity(object.len());
    for (entity_id, item) in object {
        let summary = parse_summary_value(entity_id, item)?;
        if summaries.insert(entity_id.clone(), summary).is_some() {
            return Err(crate::error::IndexError::Other(format!(
                "duplicate entity_id in batched summary response: {}",
                entity_id
            )));
        }
    }

    if summaries.is_empty() {
        return Err(crate::error::IndexError::Other(
            "batched summary response did not contain any summaries".into(),
        ));
    }

    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{ChizuStore, Config, Entity, EntityKind, Provider, ProviderError};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    struct MockProvider {
        responses: HashMap<String, String>,
        calls: AtomicUsize,
    }

    impl Provider for MockProvider {
        fn complete(
            &self,
            prompt: &str,
            _max_tokens: Option<u32>,
        ) -> std::result::Result<String, ProviderError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let key = blake3::hash(prompt.as_bytes()).to_string();
            if let Some(response) = self.responses.get(&key).cloned() {
                return Ok(response);
            }

            let entity_ids = prompt_entity_ids(prompt);
            if entity_ids.len() > 1 {
                let summaries = entity_ids
                    .iter()
                    .map(|entity_id| {
                        serde_json::json!({
                            "entity_id": entity_id,
                            "short_summary": format!("summary for {}", entity_id),
                            "detailed_summary": format!("details for {}", entity_id),
                            "keywords": ["default"],
                        })
                    })
                    .collect::<Vec<_>>();
                return Ok(serde_json::json!({ "summaries": summaries }).to_string());
            }

            Ok(r#"{"short_summary": "default summary", "detailed_summary": "default detailed", "keywords": ["default"]}"#.to_string())
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
        assert_eq!(
            summary.detailed_summary,
            Some("This function tests things.".to_string())
        );
        assert_eq!(
            summary.keywords,
            Some(vec!["test".to_string(), "rust".to_string()])
        );
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
            .with_lines(1, 1)
            .with_exported(true);
        store.insert_entity(&entity).unwrap();

        let provider = MockProvider {
            responses: HashMap::new(),
            calls: AtomicUsize::new(0),
        };
        let config = SummaryConfig::default();
        let summarizer = Summarizer::new(&provider, &config);

        let stats1 = summarizer.run(&store, &repo_root).unwrap();
        assert_eq!(stats1.generated, 1);
        assert_eq!(stats1.skipped, 0);

        let summary = store
            .get_summary("symbol::src/lib.rs::foo")
            .unwrap()
            .unwrap();
        assert_eq!(summary.short_summary, "default summary");
        assert_eq!(
            summary.detailed_summary,
            Some("default detailed".to_string())
        );
        assert_eq!(summary.keywords, Some(vec!["default".to_string()]));
        assert!(summary.source_hash.is_some());

        // Re-run should skip unchanged entity.
        let stats2 = summarizer.run(&store, &repo_root).unwrap();
        assert_eq!(stats2.generated, 0);
        assert_eq!(stats2.skipped, 1);
    }

    #[test]
    fn test_summarizer_batches_requests() {
        let (store, temp_dir) = create_test_store();
        let repo_root = temp_dir.path().join("repo");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(
            repo_root.join("src/lib.rs"),
            "pub fn foo() {}\npub fn bar() {}\npub fn baz() {}\n",
        )
        .unwrap();

        for (name, line) in [("foo", 1), ("bar", 2), ("baz", 3)] {
            let entity = Entity::new(
                format!("symbol::src/lib.rs::{name}"),
                EntityKind::Symbol,
                name,
            )
            .with_path("src/lib.rs")
            .with_lines(line, line)
            .with_exported(true);
            store.insert_entity(&entity).unwrap();
        }

        let provider = MockProvider {
            responses: HashMap::new(),
            calls: AtomicUsize::new(0),
        };
        let config = SummaryConfig {
            batch_size: Some(2),
            concurrency: Some(1),
            ..SummaryConfig::default()
        };
        let summarizer = Summarizer::new(&provider, &config);

        let stats = summarizer.run(&store, &repo_root).unwrap();
        assert_eq!(stats.generated, 3);
        assert_eq!(provider.calls.load(Ordering::Relaxed), 2);
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
        assert_eq!(
            summary.keywords,
            Some(vec!["std".to_string(), "Path".to_string()])
        );
    }

    #[test]
    fn test_parse_truncated_json_missing_closing_brace() {
        // LLM output cut off after array — missing `}`.
        let response = r#"{
  "short_summary": "A summary.",
  "keywords": ["a", "b"]"#;
        let summary = parse_summary_response("e1", response).unwrap();
        assert_eq!(summary.short_summary, "A summary.");
        assert_eq!(
            summary.keywords,
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn test_parse_truncated_json_mid_value() {
        // Truncated inside a string value — unrecoverable.
        let response = r#"{"short_summary": "Trunca"#;
        assert!(parse_summary_response("e1", response).is_err());
    }

    #[test]
    fn test_parse_batch_summary_response() {
        let response = r#"{
  "summaries": [
    {
      "entity_id": "e1",
      "short_summary": "First",
      "detailed_summary": "First detail",
      "keywords": ["a"]
    },
    {
      "entity_id": "e2",
      "short_summary": "Second",
      "keywords": ["b", "c"]
    }
  ]
}"#;
        let summaries = parse_batch_summary_response(response).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries["e1"].short_summary, "First");
        assert_eq!(
            summaries["e2"].keywords,
            Some(vec!["b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn test_chunk_ranges_respects_batch_size() {
        let ranges = chunk_ranges(10, 4);
        assert_eq!(ranges, vec![0..4, 4..8, 8..10]);
    }

    #[test]
    fn test_exact_batch_planner_respects_hard_cap() {
        let planner = ExactBatchPlanner {
            tokenizer: llama3_tokenizer(),
            context_window: 10_000,
            safety_margin_tokens: 0,
        };
        let items = vec![
            SummaryWork {
                entity_id: "e1".into(),
                prompt_input: "Entity ID: e1\nCode:\n```\nfn a() {}\n```".into(),
                source_hash: "h1".into(),
            },
            SummaryWork {
                entity_id: "e2".into(),
                prompt_input: "Entity ID: e2\nCode:\n```\nfn b() {}\n```".into(),
                source_hash: "h2".into(),
            },
            SummaryWork {
                entity_id: "e3".into(),
                prompt_input: "Entity ID: e3\nCode:\n```\nfn c() {}\n```".into(),
                source_hash: "h3".into(),
            },
        ];

        let ranges = planner.plan_batches(&items, 2, Some(64));
        assert_eq!(ranges, vec![0..2, 2..3]);
    }

    #[test]
    fn test_exact_batch_planner_splits_on_token_budget() {
        let planner = ExactBatchPlanner {
            tokenizer: llama3_tokenizer(),
            context_window: 250,
            safety_margin_tokens: 0,
        };
        let long_code = "x".repeat(2_000);
        let items = vec![
            SummaryWork {
                entity_id: "e1".into(),
                prompt_input: format!("Entity ID: e1\nCode:\n```\n{}\n```", long_code),
                source_hash: "h1".into(),
            },
            SummaryWork {
                entity_id: "e2".into(),
                prompt_input: format!("Entity ID: e2\nCode:\n```\n{}\n```", long_code),
                source_hash: "h2".into(),
            },
        ];

        let ranges = planner.plan_batches(&items, 8, Some(64));
        assert_eq!(ranges, vec![0..1, 1..2]);
    }

    fn prompt_entity_ids(prompt: &str) -> Vec<String> {
        prompt
            .lines()
            .filter_map(|line| line.strip_prefix("Entity ID: ").map(ToString::to_string))
            .collect()
    }
}
