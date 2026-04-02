use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::io::Read;
use std::ops::Range;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chizu_core::{
    ChizuStore, ComponentId, EdgeKind, Entity, EntityKind, Provider, ProviderError, Store, Summary,
    SummaryConfig,
};
use rustc_hash::FxHasher;
use tiktoken_rs::{CoreBPE, Rank};
use tracing::{debug, error, info, warn};

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
    kind: EntityKind,
    prompt_input: String,
    source_hash: String,
}

struct PreparedSummaryInput {
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
            build_single_prompt(&batch[0].prompt_input, max_tokens)
        } else {
            build_batch_prompt(batch, max_tokens, scale_max_tokens(max_tokens, batch.len()))
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
        let entities =
            collect_entities_to_summarize(store, self.config.exported_only.unwrap_or(true))?;

        if entities.is_empty() {
            debug!("No entities to summarize");
            return Ok(stats);
        }

        // Phase 1: collect work items (reads files, checks cache — single-threaded)
        let mut file_cache: HashMap<String, String> = HashMap::new();
        let mut work_items: Vec<SummaryWork> = Vec::new();

        for entity in &entities {
            let prepared =
                match build_prompt_input_for_entity(store, repo_root, entity, &mut file_cache)? {
                    Some(prepared) => prepared,
                    None => {
                        debug!("No prompt input for entity {} — skipping", entity.id);
                        stats.skipped += 1;
                        continue;
                    }
                };

            if let Some(existing) = store.get_summary(&entity.id)? {
                if existing.source_hash.as_ref() == Some(&prepared.source_hash) {
                    debug!("Summary for {} is up to date", entity.id);
                    stats.skipped += 1;
                    continue;
                }
            }

            work_items.push(SummaryWork {
                entity_id: entity.id.clone(),
                kind: entity.kind,
                prompt_input: prepared.prompt_input,
                source_hash: prepared.source_hash,
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
        let batch_ranges = plan_work_item_ranges(
            &work_items,
            batch_size,
            self.config.max_tokens,
            &batch_planner,
        );
        let exact_tokenizer = match &batch_planner {
            BatchPlanner::Exact(planner) => Some(planner.tokenizer),
            BatchPlanner::Heuristic => None,
        };
        info!(
            "  {} entities to summarize ({} batches, batch_size<= {}, concurrency={})",
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
                            let max_tokens = scale_max_tokens(self.config.max_tokens, batch.len());
                            let prompt = if batch.len() == 1 {
                                build_single_prompt(&batch[0].prompt_input, self.config.max_tokens)
                            } else {
                                build_batch_prompt(batch, self.config.max_tokens, max_tokens)
                            };
                            let prompt_tokens =
                                exact_tokenizer.map(|tokenizer| count_llama3_chat_tokens(tokenizer, &prompt));

                            info!(
                                "  summarizing batch of {} entities (prompt_tokens={:?}, requested_max_tokens={:?})",
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
                                "  llm latency: {:.1}ms ({} entities, prompt_tokens={:?}, response_tokens={:?}, requested_max_tokens={:?})",
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
                        warn!(
                            "Batched summary response omitted {} entities; falling back to singles",
                            missing.join(", ")
                        );
                        self.fallback_to_singles(store, batch, stats);
                        return;
                    }

                    for item in batch {
                        let Some(summary) = summaries.remove(&item.entity_id) else {
                            warn!(
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
                    warn!("Failed to parse batched summary response: {e}; falling back to singles");
                    self.fallback_to_singles(store, batch, stats);
                }
            },
            Err(e) => {
                warn!("Batched LLM call failed: {e}; falling back to singles");
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
        let prompt = build_single_prompt(&item.prompt_input, self.config.max_tokens);
        let prompt_tokens = exact_tokenizer_for_model(self.config.model.as_deref())
            .map(|tokenizer| count_llama3_chat_tokens(tokenizer, &prompt));
        info!(
            "  summarizing {} (prompt_tokens={:?}, requested_max_tokens={:?})",
            item.entity_id, prompt_tokens, self.config.max_tokens,
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
            elapsed, item.entity_id, prompt_tokens, response_tokens, self.config.max_tokens
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
const MAX_DOC_EXCERPT_LINES: usize = 120;
const MAX_DOC_EXCERPT_CHARS: usize = 6_000;

fn collect_entities_to_summarize(store: &ChizuStore, exported_only: bool) -> Result<Vec<Entity>> {
    let mut entities = Vec::new();

    let symbols = store.get_entities_by_kind(EntityKind::Symbol)?;
    if exported_only {
        entities.extend(symbols.into_iter().filter(|entity| entity.exported));
    } else {
        entities.extend(symbols);
    }

    entities.extend(store.get_entities_by_kind(EntityKind::Doc)?);
    entities.extend(store.get_entities_by_kind(EntityKind::Component)?);
    entities.extend(store.get_entities_by_kind(EntityKind::Repo)?);

    entities.sort_by(|left, right| {
        summary_kind_priority(left.kind)
            .cmp(&summary_kind_priority(right.kind))
            .then_with(|| left.id.cmp(&right.id))
    });
    entities.dedup_by(|left, right| left.id == right.id);

    Ok(entities)
}

fn summary_kind_priority(kind: EntityKind) -> usize {
    match kind {
        EntityKind::Repo => 0,
        EntityKind::Component => 1,
        EntityKind::Doc => 2,
        EntityKind::Symbol => 3,
        _ => 4,
    }
}

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

fn extract_file_excerpt(
    repo_root: &Path,
    path: &str,
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

    let mut excerpt = String::new();
    for line in content.lines().take(MAX_DOC_EXCERPT_LINES) {
        if !excerpt.is_empty() {
            excerpt.push('\n');
        }
        if excerpt.len() + line.len() > MAX_DOC_EXCERPT_CHARS {
            let remaining = MAX_DOC_EXCERPT_CHARS.saturating_sub(excerpt.len());
            if remaining > 0 {
                excerpt.push_str(&line[..line.len().min(remaining)]);
            }
            break;
        }
        excerpt.push_str(line);
    }

    if excerpt.trim().is_empty() {
        None
    } else {
        Some(excerpt)
    }
}

fn can_batch_summary_kind(kind: EntityKind) -> bool {
    matches!(kind, EntityKind::Symbol)
}

fn plan_work_item_ranges(
    work_items: &[SummaryWork],
    batch_size: usize,
    max_tokens: Option<u32>,
    planner: &BatchPlanner,
) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;

    while start < work_items.len() {
        if !can_batch_summary_kind(work_items[start].kind) {
            ranges.push(start..start + 1);
            start += 1;
            continue;
        }

        let mut end = start + 1;
        while end < work_items.len() && can_batch_summary_kind(work_items[end].kind) {
            end += 1;
        }

        let local_ranges = match planner {
            BatchPlanner::Exact(planner) => {
                planner.plan_batches(&work_items[start..end], batch_size, max_tokens)
            }
            BatchPlanner::Heuristic => chunk_ranges(end - start, batch_size),
        };
        ranges.extend(
            local_ranges
                .into_iter()
                .map(|range| (start + range.start)..(start + range.end)),
        );
        start = end;
    }

    ranges
}

fn build_prompt_input_for_entity(
    store: &ChizuStore,
    repo_root: &Path,
    entity: &Entity,
    file_cache: &mut HashMap<String, String>,
) -> Result<Option<PreparedSummaryInput>> {
    match entity.kind {
        EntityKind::Symbol => Ok(build_symbol_prompt_input(repo_root, entity, file_cache)),
        EntityKind::Doc => Ok(build_doc_prompt_input(repo_root, entity, file_cache)),
        EntityKind::Component => build_component_prompt_input(store, entity).map(Some),
        EntityKind::Repo => build_repo_prompt_input(store, entity).map(Some),
        _ => Ok(None),
    }
}

fn build_symbol_prompt_input(
    repo_root: &Path,
    entity: &Entity,
    file_cache: &mut HashMap<String, String>,
) -> Option<PreparedSummaryInput> {
    let path = entity.path.as_deref()?;
    let snippet = extract_snippet(
        repo_root,
        path,
        entity.line_start,
        entity.line_end,
        file_cache,
    )?;
    let prompt_input = format!(
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
        path,
        entity.line_start.unwrap_or(0),
        entity.line_end.unwrap_or(0),
        snippet
    );
    Some(PreparedSummaryInput {
        prompt_input,
        source_hash: blake3::hash(snippet.as_bytes()).to_string(),
    })
}

fn build_doc_prompt_input(
    repo_root: &Path,
    entity: &Entity,
    file_cache: &mut HashMap<String, String>,
) -> Option<PreparedSummaryInput> {
    let path = entity.path.as_deref()?;
    let excerpt = extract_file_excerpt(repo_root, path, file_cache)?;
    let prompt_input = format!(
        r#"Entity ID: {}
Entity: {}
Kind: {}
File: {}
Document excerpt:
```markdown
{}
```"#,
        entity.id, entity.name, entity.kind, path, excerpt
    );
    Some(PreparedSummaryInput {
        prompt_input,
        source_hash: blake3::hash(excerpt.as_bytes()).to_string(),
    })
}

fn build_component_prompt_input(
    store: &ChizuStore,
    entity: &Entity,
) -> Result<PreparedSummaryInput> {
    let component_id = ComponentId::parse(&entity.id).ok_or_else(|| {
        crate::error::IndexError::Other(format!("invalid component id for summary: {}", entity.id))
    })?;
    let owned = store.get_entities_by_component(&component_id)?;
    let edges = store.get_edges_from(&entity.id)?;

    let mut source_units = Vec::new();
    let mut docs = Vec::new();
    let mut symbol_count = 0usize;
    let mut test_count = 0usize;
    let mut bench_count = 0usize;

    for owned_entity in &owned {
        match owned_entity.kind {
            EntityKind::SourceUnit => source_units.push(
                owned_entity
                    .path
                    .clone()
                    .unwrap_or_else(|| owned_entity.name.clone()),
            ),
            EntityKind::Doc => docs.push(
                owned_entity
                    .path
                    .clone()
                    .unwrap_or_else(|| owned_entity.name.clone()),
            ),
            EntityKind::Symbol => symbol_count += 1,
            EntityKind::Test => test_count += 1,
            EntityKind::Bench => bench_count += 1,
            _ => {}
        }
    }

    source_units.sort();
    docs.sort();

    let dependency_names = resolve_edge_target_names(store, &edges, EdgeKind::DependsOn)?;
    let linked_doc_names = resolve_edge_target_names(store, &edges, EdgeKind::DocumentedBy)?;
    let doc_names = if linked_doc_names.is_empty() {
        docs.clone()
    } else {
        linked_doc_names
    };

    let prompt_input = format!(
        r#"Entity ID: {}
Entity: {}
Kind: {}
Path: {}
Component overview:
- Ecosystem: {}
- Source files: {}
- Symbols: {}
- Tests: {}
- Benches: {}
- Direct dependencies: {}
- Linked docs: {}
Representative source files:
{}
Documentation files:
{}"#,
        entity.id,
        entity.name,
        entity.kind,
        entity.path.as_deref().unwrap_or("."),
        component_id.ecosystem().unwrap_or("unknown"),
        source_units.len(),
        symbol_count,
        test_count,
        bench_count,
        summarize_name_list(&dependency_names, 8),
        summarize_name_list(&doc_names, 8),
        summarize_path_list(&source_units, 8),
        summarize_path_list(&docs, 6),
    );
    Ok(PreparedSummaryInput {
        source_hash: blake3::hash(prompt_input.as_bytes()).to_string(),
        prompt_input,
    })
}

fn build_repo_prompt_input(store: &ChizuStore, entity: &Entity) -> Result<PreparedSummaryInput> {
    let components = store.get_entities_by_kind(EntityKind::Component)?;
    let docs = store.get_entities_by_kind(EntityKind::Doc)?;
    let source_units = store.get_entities_by_kind(EntityKind::SourceUnit)?;
    let symbols = store.get_entities_by_kind(EntityKind::Symbol)?;
    let tests = store.get_entities_by_kind(EntityKind::Test)?;
    let benches = store.get_entities_by_kind(EntityKind::Bench)?;
    let features = store.get_entities_by_kind(EntityKind::Feature)?;

    let mut component_names = components
        .iter()
        .map(|component| component.name.clone())
        .collect::<Vec<_>>();
    component_names.sort();
    let mut doc_paths = docs
        .iter()
        .map(|doc| doc.path.clone().unwrap_or_else(|| doc.name.clone()))
        .collect::<Vec<_>>();
    doc_paths.sort();

    let prompt_input = format!(
        r#"Entity ID: {}
Entity: {}
Kind: {}
Path: {}
Repository overview:
- Components: {}
- Docs: {}
- Source files: {}
- Symbols: {}
- Tests: {}
- Benches: {}
- Features: {}
Key components:
{}
Key docs:
{}"#,
        entity.id,
        entity.name,
        entity.kind,
        entity.path.as_deref().unwrap_or("."),
        components.len(),
        docs.len(),
        source_units.len(),
        symbols.len(),
        tests.len(),
        benches.len(),
        features.len(),
        summarize_name_list(&component_names, 10),
        summarize_path_list(&doc_paths, 10),
    );
    Ok(PreparedSummaryInput {
        source_hash: blake3::hash(prompt_input.as_bytes()).to_string(),
        prompt_input,
    })
}

fn resolve_edge_target_names(
    store: &ChizuStore,
    edges: &[chizu_core::Edge],
    rel: EdgeKind,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for edge in edges.iter().filter(|edge| edge.rel == rel) {
        if let Some(target) = store.get_entity(&edge.dst_id)? {
            names.push(target.name);
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn summarize_name_list(names: &[String], limit: usize) -> String {
    if names.is_empty() {
        return "- none".to_string();
    }

    let mut lines = names
        .iter()
        .take(limit)
        .map(|name| format!("- {}", name))
        .collect::<Vec<_>>();
    if names.len() > limit {
        lines.push(format!("- …and {} more", names.len() - limit));
    }
    lines.join("\n")
}

fn summarize_path_list(paths: &[String], limit: usize) -> String {
    if paths.is_empty() {
        return "- none".to_string();
    }

    let mut lines = paths
        .iter()
        .take(limit)
        .map(|path| format!("- {}", path))
        .collect::<Vec<_>>();
    if paths.len() > limit {
        lines.push(format!("- …and {} more", paths.len() - limit));
    }
    lines.join("\n")
}

fn response_contract(
    per_item_max_tokens: Option<u32>,
    total_max_tokens: Option<u32>,
    batched: bool,
) -> String {
    let per_item = per_item_max_tokens.unwrap_or(128);
    let total = total_max_tokens.unwrap_or(per_item);
    let batch_line = if batched {
        format!(
            "- Each summary object must fit within about {} output tokens, and the full JSON response must fit within about {} output tokens.\n",
            per_item, total
        )
    } else {
        format!(
            "- The full JSON response must fit within about {} output tokens.\n",
            total
        )
    };

    format!(
        r#"Rules:
{}
- `short_summary` is required and must never be omitted.
- Keep `short_summary` to one short sentence, about 24 words max.
- Keep `detailed_summary` to 1-2 short sentences, about 60 words max.
- Return at most 4 short keywords.
- If you are running out of space, shorten `detailed_summary` and `keywords` first, but still return `short_summary`.
- Return compact JSON on a single line.
- Do not wrap the response in markdown."#,
        batch_line
    )
}

fn build_single_prompt(prompt_input: &str, max_tokens: Option<u32>) -> String {
    format!(
        r#"You are a codebase documentation assistant. Given the following repository entity, provide a concise summary.

{}

Respond with ONLY a JSON object in this exact format:
{{
  "short_summary": "one sentence summary",
  "detailed_summary": "2-3 sentence detailed description",
  "keywords": ["keyword1", "keyword2", "keyword3"]
}}

{}"#,
        prompt_input,
        response_contract(max_tokens, max_tokens, false)
    )
}

fn build_batch_prompt(
    batch: &[SummaryWork],
    per_item_max_tokens: Option<u32>,
    total_max_tokens: Option<u32>,
) -> String {
    let entities = batch
        .iter()
        .enumerate()
        .map(|(index, item)| format!("Entity {}:\n{}", index + 1, item.prompt_input))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r#"You are a codebase documentation assistant. Given the following repository entities, provide a concise summary for each one.

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
{}"#,
        entities,
        response_contract(per_item_max_tokens, total_max_tokens, true)
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
        .or_else(|| object.get("summary").and_then(|v| v.as_str()))
        .or_else(|| object.get("description").and_then(|v| v.as_str()))
        .or_else(|| object.get("detailed_summary").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            crate::error::IndexError::Other(format!(
                "missing short_summary in response for {}",
                entity_id
            ))
        })?;

    let short = compact_summary_sentence(short);
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

fn normalize_response_text(response: &str) -> String {
    if response.trim().starts_with("```") {
        response
            .lines()
            .skip_while(|l| l.trim().starts_with("```"))
            .take_while(|l| !l.trim().starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    } else {
        response.trim().to_string()
    }
}

fn parse_summary_response(entity_id: &str, response: &str) -> Result<Summary> {
    match parse_response_json(response) {
        Ok(value) => match parse_summary_value(entity_id, &value) {
            Ok(summary) => Ok(summary),
            Err(err) => salvage_summary_from_raw(entity_id, response).ok_or(err),
        },
        Err(err) => salvage_summary_from_raw(entity_id, response).ok_or(err),
    }
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

fn extract_json_string_field(raw: &str, key: &str) -> Option<String> {
    let marker = format!(r#""{}""#, key);
    let start = raw.find(&marker)? + marker.len();
    let after_key = raw.get(start..)?;
    let colon_offset = after_key.find(':')?;
    let after_colon = after_key.get(colon_offset + 1..)?.trim_start();
    let mut chars = after_colon.chars();
    if chars.next()? != '"' {
        return None;
    }

    let mut value = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            value.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some(value.trim().to_string()),
            other => value.push(other),
        }
    }

    (!value.trim().is_empty()).then(|| value.trim().to_string())
}

fn compact_summary_sentence(text: &str) -> String {
    let trimmed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return String::new();
    }

    let sentence_end = trimmed
        .char_indices()
        .find_map(|(idx, ch)| matches!(ch, '.' | '!' | '?' | '\n').then_some(idx + ch.len_utf8()));
    let sentence = sentence_end
        .and_then(|end| trimmed.get(..end))
        .unwrap_or(trimmed.as_str())
        .trim();

    let words = sentence.split_whitespace().take(24).collect::<Vec<_>>();
    let mut compact = words.join(" ");
    if compact.is_empty() {
        compact = trimmed;
    }
    if !compact.ends_with('.') && !compact.ends_with('!') && !compact.ends_with('?') {
        compact.push('.');
    }
    compact
}

fn salvage_summary_from_raw(entity_id: &str, response: &str) -> Option<Summary> {
    let normalized = normalize_response_text(response);
    let short = extract_json_string_field(&normalized, "short_summary")
        .or_else(|| extract_json_string_field(&normalized, "summary"))
        .or_else(|| extract_json_string_field(&normalized, "description"))
        .or_else(|| extract_json_string_field(&normalized, "detailed_summary"))
        .or_else(|| {
            let plain = normalized.trim();
            if plain.is_empty() || plain.starts_with('{') {
                None
            } else {
                Some(plain.to_string())
            }
        })?;

    let short = compact_summary_sentence(&short);
    if short.is_empty() {
        return None;
    }

    let mut summary = Summary::new(entity_id, short);
    if let Some(detailed) = extract_json_string_field(&normalized, "detailed_summary") {
        let detailed = detailed.trim().to_string();
        if !detailed.is_empty() && detailed != summary.short_summary {
            summary = summary.with_detailed(detailed);
        }
    }
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{
        ChizuStore, ComponentId, Config, Edge, EdgeKind, Entity, EntityKind, Provider,
        ProviderError,
    };
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
        assert_eq!(summary.short_summary, "A test function.");
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
        assert_eq!(summary.short_summary, "wrapped.");
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
        assert_eq!(summary.short_summary, "default summary.");
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
    fn test_summarizer_generates_repo_component_and_doc_summaries() {
        let (store, temp_dir) = create_test_store();
        let repo_root = temp_dir.path().join("repo");
        std::fs::create_dir_all(repo_root.join("crate-a/src")).unwrap();
        std::fs::write(
            repo_root.join("README.md"),
            "# Fixture Repo\n\nThis repository indexes a codebase into a local graph.\n",
        )
        .unwrap();
        std::fs::write(
            repo_root.join("crate-a/src/lib.rs"),
            "pub fn handler() {}\n\n#[test]\nfn handler_test() {}\n",
        )
        .unwrap();

        let repo = Entity::new("repo::.", EntityKind::Repo, "repo")
            .with_path(".")
            .with_exported(true);
        let component = Entity::new(
            "component::cargo::crate-a",
            EntityKind::Component,
            "crate-a",
        )
        .with_path("crate-a")
        .with_exported(true);
        let doc = Entity::new("doc::README.md", EntityKind::Doc, "README.md")
            .with_path("README.md")
            .with_exported(true);
        let source_unit = Entity::new(
            "source_unit::crate-a/src/lib.rs",
            EntityKind::SourceUnit,
            "lib.rs",
        )
        .with_path("crate-a/src/lib.rs")
        .with_component(ComponentId::new("cargo", "crate-a"))
        .with_exported(true);
        let symbol = Entity::new(
            "symbol::crate-a/src/lib.rs::handler",
            EntityKind::Symbol,
            "handler",
        )
        .with_path("crate-a/src/lib.rs")
        .with_lines(1, 1)
        .with_component(ComponentId::new("cargo", "crate-a"))
        .with_exported(true);
        let test = Entity::new(
            "test::crate-a/src/lib.rs::handler_test",
            EntityKind::Test,
            "handler_test",
        )
        .with_path("crate-a/src/lib.rs")
        .with_lines(4, 4)
        .with_component(ComponentId::new("cargo", "crate-a"))
        .with_exported(true);

        for entity in [&repo, &component, &doc, &source_unit, &symbol, &test] {
            store.insert_entity(entity).unwrap();
        }
        for edge in [
            Edge::new("repo::.", EdgeKind::Contains, "component::cargo::crate-a"),
            Edge::new("repo::.", EdgeKind::DocumentedBy, "doc::README.md"),
            Edge::new(
                "component::cargo::crate-a",
                EdgeKind::Contains,
                "source_unit::crate-a/src/lib.rs",
            ),
            Edge::new(
                "source_unit::crate-a/src/lib.rs",
                EdgeKind::Defines,
                "symbol::crate-a/src/lib.rs::handler",
            ),
            Edge::new(
                "source_unit::crate-a/src/lib.rs",
                EdgeKind::TestedBy,
                "test::crate-a/src/lib.rs::handler_test",
            ),
        ] {
            store.insert_edge(&edge).unwrap();
        }

        let provider = MockProvider {
            responses: HashMap::new(),
            calls: AtomicUsize::new(0),
        };
        let config = SummaryConfig::default();
        let summarizer = Summarizer::new(&provider, &config);

        let stats = summarizer.run(&store, &repo_root).unwrap();
        assert!(stats.generated >= 4);
        assert!(store.get_summary("repo::.").unwrap().is_some());
        assert!(
            store
                .get_summary("component::cargo::crate-a")
                .unwrap()
                .is_some()
        );
        assert!(store.get_summary("doc::README.md").unwrap().is_some());
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
        // Truncated inside a string value — salvage the short summary when possible.
        let response = r#"{"short_summary": "Trunca"#;
        let summary = parse_summary_response("e1", response).unwrap();
        assert_eq!(summary.short_summary, "Trunca.");
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
        assert_eq!(summaries["e1"].short_summary, "First.");
        assert_eq!(
            summaries["e2"].keywords,
            Some(vec!["b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn test_prompt_contract_mentions_token_budget_and_required_short_summary() {
        let single = build_single_prompt("Entity ID: e1\nKind: doc", Some(128));
        assert!(single.contains("128 output tokens"));
        assert!(single.contains("`short_summary` is required and must never be omitted"));

        let batch = build_batch_prompt(
            &[SummaryWork {
                entity_id: "e1".into(),
                kind: EntityKind::Doc,
                prompt_input: "Entity ID: e1\nKind: doc".into(),
                source_hash: "h1".into(),
            }],
            Some(128),
            Some(384),
        );
        assert!(batch.contains("128 output tokens"));
        assert!(batch.contains("384 output tokens"));
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
                kind: EntityKind::Symbol,
                prompt_input: "Entity ID: e1\nCode:\n```\nfn a() {}\n```".into(),
                source_hash: "h1".into(),
            },
            SummaryWork {
                entity_id: "e2".into(),
                kind: EntityKind::Symbol,
                prompt_input: "Entity ID: e2\nCode:\n```\nfn b() {}\n```".into(),
                source_hash: "h2".into(),
            },
            SummaryWork {
                entity_id: "e3".into(),
                kind: EntityKind::Symbol,
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
                kind: EntityKind::Symbol,
                prompt_input: format!("Entity ID: e1\nCode:\n```\n{}\n```", long_code),
                source_hash: "h1".into(),
            },
            SummaryWork {
                entity_id: "e2".into(),
                kind: EntityKind::Symbol,
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
