use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chizu_core::{ChizuStore, Config, Provider, ProviderError};
use chizu_index::IndexPipeline;
use tempfile::TempDir;

const PERF_BATCH_SIZE: usize = 8;
const PER_CALL_DELAY_MS: u64 = 10;

struct DelayedSummaryProvider {
    calls: AtomicUsize,
    per_call_delay: Duration,
}

impl DelayedSummaryProvider {
    fn new(per_call_delay: Duration) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            per_call_delay,
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl Provider for DelayedSummaryProvider {
    async fn complete(
        &self,
        prompt: &str,
        _max_tokens: Option<u32>,
    ) -> std::result::Result<String, ProviderError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(self.per_call_delay).await;

        let entity_ids = prompt_entity_ids(prompt);
        if entity_ids.len() > 1 {
            let summaries = entity_ids
                .iter()
                .map(|entity_id| {
                    serde_json::json!({
                        "entity_id": entity_id,
                        "short_summary": format!("summary for {}", entity_id),
                        "detailed_summary": format!("details for {}", entity_id),
                        "keywords": ["perf"],
                    })
                })
                .collect::<Vec<_>>();
            return Ok(serde_json::json!({ "summaries": summaries }).to_string());
        }

        Ok(
            r#"{"short_summary": "default summary", "detailed_summary": "default detailed", "keywords": ["perf"]}"#
                .to_string(),
        )
    }

    async fn embed(&self, _texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
        Ok(Vec::new())
    }
}

struct PerfRun {
    elapsed: Duration,
    summaries_generated: usize,
    summary_calls: usize,
}

#[tokio::test]
#[ignore = "performance comparison against the current workspace"]
async fn compare_repo_index_time_for_summary_batch_sizes() {
    let repo_root = workspace_root();

    let single = run_index(repo_root, 1).await;
    let batched = run_index(repo_root, PERF_BATCH_SIZE).await;

    println!(
        "repo={} batch_size=1 elapsed_ms={:.1} summaries={} calls={}",
        repo_root.display(),
        single.elapsed.as_secs_f64() * 1000.0,
        single.summaries_generated,
        single.summary_calls
    );
    println!(
        "repo={} batch_size={} elapsed_ms={:.1} summaries={} calls={}",
        repo_root.display(),
        PERF_BATCH_SIZE,
        batched.elapsed.as_secs_f64() * 1000.0,
        batched.summaries_generated,
        batched.summary_calls
    );

    assert!(single.summaries_generated > 0);
    assert_eq!(single.summaries_generated, batched.summaries_generated);
    assert!(batched.summary_calls < single.summary_calls);
    assert!(batched.elapsed < single.elapsed);
}

async fn run_index(repo_root: &Path, batch_size: usize) -> PerfRun {
    let temp_dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.summary.provider = Some("ollama".to_string());
    config.summary.model = Some("llama3:8b".to_string());
    config.summary.batch_size = Some(batch_size);
    config.summary.concurrency = Some(1);
    config.embedding.provider = None;

    let provider = DelayedSummaryProvider::new(Duration::from_millis(PER_CALL_DELAY_MS));
    let store = ChizuStore::open(temp_dir.path(), &config).unwrap();

    let start = Instant::now();
    let stats = IndexPipeline::run(repo_root, &store, &config, Some(&provider))
        .await
        .unwrap();
    let elapsed = start.elapsed();

    let expected_calls = div_ceil(stats.summaries_generated, batch_size);
    let summary_calls = provider.call_count();
    assert_eq!(summary_calls, expected_calls);

    store.close().unwrap();

    PerfRun {
        elapsed,
        summaries_generated: stats.summaries_generated,
        summary_calls,
    }
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root should be the parent of chizu-index")
}

fn prompt_entity_ids(prompt: &str) -> Vec<String> {
    prompt
        .lines()
        .filter_map(|line| line.strip_prefix("Entity ID: ").map(ToString::to_string))
        .collect()
}

fn div_ceil(value: usize, divisor: usize) -> usize {
    value.div_ceil(divisor)
}
