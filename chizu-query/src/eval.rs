use std::collections::{HashMap, HashSet};

use chizu_core::{Config, Provider, Reranker, Store, TaskCategory};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::pipeline::{SearchOptions, SearchPipeline};

// ---------------------------------------------------------------------------
// Benchmark format (loaded from TOML)
// ---------------------------------------------------------------------------

/// A benchmark file definition.
#[derive(Debug, Deserialize)]
pub struct Benchmark {
    /// Format version (currently 1).
    pub version: u32,
    /// Labeled queries to evaluate.
    pub queries: Vec<BenchmarkQuery>,
}

/// A single labeled query in the benchmark.
#[derive(Debug, Deserialize)]
pub struct BenchmarkQuery {
    /// The search query text.
    pub text: String,
    /// Query bucket: "identifier", "concept", or "task".
    pub bucket: String,
    /// Optional task category override (auto-classified if absent).
    pub category: Option<String>,
    /// Entity IDs that are the primary relevant results.
    pub relevant: Vec<String>,
    /// Entity IDs that are acceptable supporting results.
    #[serde(default)]
    pub acceptable: Vec<String>,
}

// ---------------------------------------------------------------------------
// Evaluation output
// ---------------------------------------------------------------------------

/// Full evaluation result for a benchmark run.
#[derive(Debug, Serialize)]
pub struct EvalOutput {
    pub overall: EvalMetrics,
    pub by_bucket: HashMap<String, EvalMetrics>,
    pub queries: Vec<QueryResult>,
}

/// Aggregated IR metrics.
#[derive(Debug, Serialize, Default, Clone)]
pub struct EvalMetrics {
    pub recall_at_5: f64,
    pub mrr_at_10: f64,
    pub ndcg_at_10: f64,
    pub noise_tail_rate: f64,
    pub query_count: usize,
}

/// Per-query evaluation result.
#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub text: String,
    pub bucket: String,
    pub results: Vec<String>,
    pub recall_at_5: f64,
    pub mrr_at_10: f64,
    pub ndcg_at_10: f64,
    pub noise_tail_rate: f64,
}

// ---------------------------------------------------------------------------
// Metric functions
// ---------------------------------------------------------------------------

/// Recall@k: fraction of relevant items found in the top-k results.
pub fn recall_at_k(results: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    if relevant.is_empty() {
        return 0.0;
    }
    let hits = results
        .iter()
        .take(k)
        .filter(|r| relevant.contains(*r))
        .count();
    hits as f64 / relevant.len() as f64
}

/// MRR@k: reciprocal rank of the first relevant item in top-k (0 if none).
pub fn mrr_at_k(results: &[String], relevant: &HashSet<String>, k: usize) -> f64 {
    for (i, result) in results.iter().take(k).enumerate() {
        if relevant.contains(result) {
            return 1.0 / (i + 1) as f64;
        }
    }
    0.0
}

/// nDCG@k: normalized discounted cumulative gain.
/// Relevance levels: relevant=2, acceptable=1, unknown=0.
pub fn ndcg_at_k(
    results: &[String],
    relevant: &HashSet<String>,
    acceptable: &HashSet<String>,
    k: usize,
) -> f64 {
    let dcg = dcg(results, relevant, acceptable, k);
    let ideal = ideal_dcg(relevant.len(), acceptable.len(), k);
    if ideal == 0.0 { 0.0 } else { dcg / ideal }
}

fn dcg(
    results: &[String],
    relevant: &HashSet<String>,
    acceptable: &HashSet<String>,
    k: usize,
) -> f64 {
    results
        .iter()
        .take(k)
        .enumerate()
        .map(|(i, r)| {
            let rel = if relevant.contains(r) {
                2.0
            } else if acceptable.contains(r) {
                1.0
            } else {
                0.0
            };
            (2.0_f64.powf(rel) - 1.0) / (i as f64 + 2.0).log2()
        })
        .sum()
}

fn ideal_dcg(n_relevant: usize, n_acceptable: usize, k: usize) -> f64 {
    let n_rel = n_relevant.min(k);
    let n_acc = n_acceptable.min(k.saturating_sub(n_relevant));
    (0..n_rel)
        .map(|i| 3.0 / (i as f64 + 2.0).log2())
        .chain((0..n_acc).map(|j| 1.0 / ((n_rel + j) as f64 + 2.0).log2()))
        .sum()
}

/// Noise-tail rate: fraction of results after `after_rank` that are neither
/// relevant nor acceptable.
pub fn noise_tail_rate(
    results: &[String],
    relevant: &HashSet<String>,
    acceptable: &HashSet<String>,
    after_rank: usize,
) -> f64 {
    let tail_len = results.len().saturating_sub(after_rank);
    if tail_len == 0 {
        return 0.0;
    }
    let noise = results
        .iter()
        .skip(after_rank)
        .filter(|r| !relevant.contains(*r) && !acceptable.contains(*r))
        .count();
    noise as f64 / tail_len as f64
}

// ---------------------------------------------------------------------------
// Evaluation runner
// ---------------------------------------------------------------------------

/// Run the full benchmark evaluation.
pub async fn evaluate(
    benchmark: &Benchmark,
    store: &dyn Store,
    config: &Config,
    provider: Option<&dyn Provider>,
    reranker: Option<&dyn Reranker>,
    limit: usize,
) -> Result<EvalOutput> {
    let mut query_results = Vec::new();

    for bq in &benchmark.queries {
        let category = bq
            .category
            .as_ref()
            .and_then(|c| c.parse::<TaskCategory>().ok());

        let options = SearchOptions {
            limit,
            show_all: true, // No cutoff during eval — evaluate raw ranking
            verbose: false,
        };

        let plan =
            SearchPipeline::run(store, &bq.text, category, &options, config, provider, reranker)
                .await?;

        let results: Vec<String> = plan.entries.iter().map(|e| e.entity_id.clone()).collect();
        let relevant: HashSet<String> = bq.relevant.iter().cloned().collect();
        let acceptable: HashSet<String> = bq.acceptable.iter().cloned().collect();

        query_results.push(QueryResult {
            text: bq.text.clone(),
            bucket: bq.bucket.clone(),
            recall_at_5: recall_at_k(&results, &relevant, 5),
            mrr_at_10: mrr_at_k(&results, &relevant, 10),
            ndcg_at_10: ndcg_at_k(&results, &relevant, &acceptable, 10),
            noise_tail_rate: noise_tail_rate(&results, &relevant, &acceptable, 5),
            results,
        });
    }

    let all_refs: Vec<&QueryResult> = query_results.iter().collect();
    let overall = aggregate(&all_refs);

    let mut buckets: HashMap<String, Vec<&QueryResult>> = HashMap::new();
    for qr in &query_results {
        buckets.entry(qr.bucket.clone()).or_default().push(qr);
    }
    let by_bucket = buckets
        .into_iter()
        .map(|(bucket, refs)| (bucket, aggregate(&refs)))
        .collect();

    Ok(EvalOutput {
        overall,
        by_bucket,
        queries: query_results,
    })
}

fn aggregate(results: &[&QueryResult]) -> EvalMetrics {
    if results.is_empty() {
        return EvalMetrics::default();
    }
    let n = results.len() as f64;
    EvalMetrics {
        recall_at_5: results.iter().map(|r| r.recall_at_5).sum::<f64>() / n,
        mrr_at_10: results.iter().map(|r| r.mrr_at_10).sum::<f64>() / n,
        ndcg_at_10: results.iter().map(|r| r.ndcg_at_10).sum::<f64>() / n,
        noise_tail_rate: results.iter().map(|r| r.noise_tail_rate).sum::<f64>() / n,
        query_count: results.len(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn results(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_recall_at_k_all_found() {
        let r = results(&["a", "b", "c"]);
        let rel = set(&["a", "b"]);
        assert!((recall_at_k(&r, &rel, 5) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_recall_at_k_partial() {
        let r = results(&["a", "x", "y", "z", "w"]);
        let rel = set(&["a", "b"]);
        assert!((recall_at_k(&r, &rel, 5) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_recall_at_k_none_found() {
        let r = results(&["x", "y", "z"]);
        let rel = set(&["a", "b"]);
        assert!((recall_at_k(&r, &rel, 5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_recall_at_k_empty_relevant() {
        let r = results(&["a", "b"]);
        let rel = set(&[]);
        assert!((recall_at_k(&r, &rel, 5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_mrr_at_k_first() {
        let r = results(&["a", "b", "c"]);
        let rel = set(&["a"]);
        assert!((mrr_at_k(&r, &rel, 10) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_mrr_at_k_third() {
        let r = results(&["x", "y", "a"]);
        let rel = set(&["a"]);
        assert!((mrr_at_k(&r, &rel, 10) - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_mrr_at_k_not_found() {
        let r = results(&["x", "y", "z"]);
        let rel = set(&["a"]);
        assert!((mrr_at_k(&r, &rel, 10) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_ndcg_perfect_ranking() {
        // All relevant at top
        let r = results(&["a", "b", "x"]);
        let rel = set(&["a", "b"]);
        let acc = set(&[]);
        let score = ndcg_at_k(&r, &rel, &acc, 3);
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_ndcg_inverted_ranking() {
        // Relevant items last
        let r = results(&["x", "y", "a"]);
        let rel = set(&["a"]);
        let acc = set(&[]);
        let score = ndcg_at_k(&r, &rel, &acc, 3);
        assert!(score < 1.0);
        assert!(score > 0.0);
    }

    #[test]
    fn test_noise_tail_rate_all_noise() {
        let r = results(&["a", "b", "c", "d", "e", "x", "y", "z"]);
        let rel = set(&["a"]);
        let acc = set(&[]);
        // After rank 5: ["x", "y", "z"] — all noise
        assert!((noise_tail_rate(&r, &rel, &acc, 5) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_noise_tail_rate_no_tail() {
        let r = results(&["a", "b", "c"]);
        let rel = set(&["a"]);
        let acc = set(&[]);
        assert!((noise_tail_rate(&r, &rel, &acc, 5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_noise_tail_rate_mixed() {
        let r = results(&["a", "b", "c", "d", "e", "f", "x"]);
        let rel = set(&["a", "f"]);
        let acc = set(&[]);
        // After rank 5: ["f", "x"] — f is relevant, x is noise → 0.5
        assert!((noise_tail_rate(&r, &rel, &acc, 5) - 0.5).abs() < 0.001);
    }
}
