use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use chizu_core::{
    ChizuStore, Config, Entity, EntityKind, OpenAiProvider, Provider, ProviderError, Store,
};
use chizu_index::{IndexPipeline, summarizer::Summarizer};
use tempfile::TempDir;

const BATCH_SIZES: [usize; 4] = [1, 2, 4, 8];
const SAMPLE_SIZE: usize = 16;

struct CountingProvider<P> {
    inner: P,
    completion_calls: AtomicUsize,
    embedding_calls: AtomicUsize,
}

impl<P> CountingProvider<P> {
    fn new(inner: P) -> Self {
        Self {
            inner,
            completion_calls: AtomicUsize::new(0),
            embedding_calls: AtomicUsize::new(0),
        }
    }

    fn completion_calls(&self) -> usize {
        self.completion_calls.load(Ordering::Relaxed)
    }

    fn embedding_calls(&self) -> usize {
        self.embedding_calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl<P: Provider> Provider for CountingProvider<P> {
    async fn complete(
        &self,
        prompt: &str,
        max_tokens: Option<u32>,
    ) -> std::result::Result<String, ProviderError> {
        self.completion_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.complete(prompt, max_tokens).await
    }

    async fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
        self.embedding_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.embed(texts).await
    }
}

struct LiveRun {
    batch_size: usize,
    elapsed_ms: f64,
    summaries_generated: usize,
    summary_calls: usize,
    embedding_calls: usize,
}

#[tokio::test]
#[ignore = "requires a local Ollama server and can take many minutes"]
async fn live_ollama_repo_batch_sweep() {
    let repo_root = workspace_root();
    let mut runs = Vec::new();
    let first = run_live(repo_root, 1).await;
    let expected_summaries = first.summaries_generated;
    runs.push(first);
    for batch_size in BATCH_SIZES.into_iter().skip(1) {
        let run = run_live(repo_root, batch_size).await;
        assert_eq!(run.summaries_generated, expected_summaries);
        runs.push(run);
    }

    for run in &runs {
        println!(
            "repo={} batch_size={} elapsed_ms={:.1} summaries={} summary_calls={} avg_symbols_per_call={:.2} embedding_calls={}",
            repo_root.display(),
            run.batch_size,
            run.elapsed_ms,
            run.summaries_generated,
            run.summary_calls,
            run.summaries_generated as f64 / run.summary_calls as f64,
            run.embedding_calls,
        );
    }

    let best = runs
        .iter()
        .min_by(|a, b| a.elapsed_ms.total_cmp(&b.elapsed_ms))
        .expect("at least one run");

    println!(
        "best_batch_size={} best_elapsed_ms={:.1}",
        best.batch_size, best.elapsed_ms
    );
}

#[tokio::test]
#[ignore = "requires a local Ollama server but completes much faster than the full repo sweep"]
async fn live_ollama_sampled_summary_batch_sweep() {
    let repo_root = workspace_root();
    let sample_entities = prepare_symbol_sample(repo_root, SAMPLE_SIZE).await;
    assert!(!sample_entities.is_empty(), "expected sampled symbols");

    let mut runs = Vec::new();
    for batch_size in BATCH_SIZES {
        runs.push(run_live_sample(repo_root, &sample_entities, batch_size).await);
    }

    for run in &runs {
        println!(
            "repo={} sample_symbols={} batch_size={} elapsed_ms={:.1} summaries={} summary_calls={} avg_symbols_per_call={:.2}",
            repo_root.display(),
            sample_entities.len(),
            run.batch_size,
            run.elapsed_ms,
            run.summaries_generated,
            run.summary_calls,
            run.summaries_generated as f64 / run.summary_calls as f64,
        );
    }

    let best = runs
        .iter()
        .min_by(|a, b| a.elapsed_ms.total_cmp(&b.elapsed_ms))
        .expect("at least one run");
    println!(
        "sample_symbols={} best_batch_size={} best_elapsed_ms={:.1}",
        sample_entities.len(),
        best.batch_size,
        best.elapsed_ms
    );
}

async fn run_live(repo_root: &Path, batch_size: usize) -> LiveRun {
    let mut config = Config::default();
    config
        .index
        .exclude_patterns
        .push("**/.chizu/**".to_string());
    config
        .index
        .exclude_patterns
        .push("**/.claude/**".to_string());
    config
        .index
        .exclude_patterns
        .push("**/.crush/**".to_string());
    config.summary.provider = Some("ollama".to_string());
    config.summary.model = Some("llama3:8b".to_string());
    config.summary.batch_size = Some(batch_size);
    config.summary.concurrency = Some(1);
    config.embedding.provider = None;

    let provider_name = config
        .summary
        .provider
        .as_ref()
        .expect("summary provider should be configured");
    let provider_config = config
        .providers
        .get(provider_name)
        .expect("provider config should exist");
    let provider = OpenAiProvider::new(
        provider_config,
        config
            .summary
            .model
            .clone()
            .expect("summary model should be configured"),
        config.embedding.model.clone().unwrap_or_default(),
        config.embedding.dimensions,
    )
    .expect("provider should initialize");
    let provider = CountingProvider::new(provider);

    // Warm the local model once before timing the full index run.
    provider
        .complete("Reply with ok.", Some(8))
        .await
        .expect("Ollama warmup should succeed");

    let temp_dir = TempDir::new().unwrap();
    let store = ChizuStore::open(temp_dir.path(), &config).unwrap();

    let start = Instant::now();
    let stats = IndexPipeline::run(repo_root, &store, &config, Some(&provider))
        .await
        .unwrap();
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    store.close().unwrap();

    LiveRun {
        batch_size,
        elapsed_ms,
        summaries_generated: stats.summaries_generated,
        summary_calls: provider.completion_calls().saturating_sub(1),
        embedding_calls: provider.embedding_calls(),
    }
}

async fn run_live_sample(
    repo_root: &Path,
    sample_entities: &[Entity],
    batch_size: usize,
) -> LiveRun {
    let mut config = Config::default();
    config.summary.provider = Some("ollama".to_string());
    config.summary.model = Some("llama3:8b".to_string());
    config.summary.batch_size = Some(batch_size);
    config.summary.concurrency = Some(1);
    config.embedding.provider = None;

    let provider = build_counting_provider(&config);

    // Warm the local model once before timing the sampled summary run.
    provider
        .complete("Reply with ok.", Some(8))
        .await
        .expect("Ollama warmup should succeed");

    let temp_dir = TempDir::new().unwrap();
    let store = ChizuStore::open(temp_dir.path(), &config).unwrap();
    store
        .in_transaction(|tx| {
            for entity in sample_entities {
                tx.insert_entity(entity)?;
            }
            Ok(())
        })
        .unwrap();

    let start = Instant::now();
    let stats = Summarizer::new(&provider, &config.summary)
        .run(&store, repo_root)
        .await
        .unwrap();
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    store.close().unwrap();

    LiveRun {
        batch_size,
        elapsed_ms,
        summaries_generated: stats.generated,
        summary_calls: provider.completion_calls().saturating_sub(1),
        embedding_calls: provider.embedding_calls(),
    }
}

async fn prepare_symbol_sample(repo_root: &Path, sample_size: usize) -> Vec<Entity> {
    let mut config = Config::default();
    config
        .index
        .exclude_patterns
        .push("**/.chizu/**".to_string());
    config
        .index
        .exclude_patterns
        .push("**/.claude/**".to_string());
    config
        .index
        .exclude_patterns
        .push("**/.crush/**".to_string());
    config.summary.provider = None;
    config.embedding.provider = None;

    let temp_dir = TempDir::new().unwrap();
    let store = ChizuStore::open(temp_dir.path(), &config).unwrap();
    IndexPipeline::run(repo_root, &store, &config, None)
        .await
        .unwrap();

    let mut symbols = store.get_entities_by_kind(EntityKind::Symbol).unwrap();
    symbols.retain(|entity| entity.exported);
    symbols.sort_by(|a, b| a.id.cmp(&b.id));
    symbols.truncate(sample_size);

    store.close().unwrap();
    symbols
}

fn build_counting_provider(config: &Config) -> CountingProvider<OpenAiProvider> {
    let provider_name = config
        .summary
        .provider
        .as_ref()
        .expect("summary provider should be configured");
    let provider_config = config
        .providers
        .get(provider_name)
        .expect("provider config should exist");
    let provider = OpenAiProvider::new(
        provider_config,
        config
            .summary
            .model
            .clone()
            .expect("summary model should be configured"),
        config.embedding.model.clone().unwrap_or_default(),
        config.embedding.dimensions,
    )
    .expect("provider should initialize");
    CountingProvider::new(provider)
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root should be the parent of chizu-index")
}
