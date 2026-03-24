//! Observability stack using rolly.
//!
//! Provides tracing, metrics, and structured logging for Grakno.

use rolly::{counter, gauge, histogram, Counter, Gauge, Histogram};
use std::sync::OnceLock;
use std::time::Duration;

/// Global metrics for the indexing pipeline.
pub struct IndexMetrics {
    pub files_indexed: Counter,
    pub files_skipped: Counter,
    pub symbols_extracted: Counter,
    pub edges_created: Counter,
    pub index_duration: Histogram,
}

impl IndexMetrics {
    fn new() -> Self {
        Self {
            files_indexed: counter("grakno.index.files_indexed", "Total files indexed"),
            files_skipped: counter(
                "grakno.index.files_skipped",
                "Files skipped due to unchanged hash",
            ),
            symbols_extracted: counter(
                "grakno.index.symbols_extracted",
                "Total symbols extracted from source",
            ),
            edges_created: counter("grakno.index.edges_created", "Total edges created in graph"),
            index_duration: histogram(
                "grakno.index.duration_seconds",
                "Time spent indexing",
                &[
                    0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
                ],
            ),
        }
    }
}

impl Default for IndexMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Global metrics for the query pipeline.
pub struct QueryMetrics {
    pub queries_total: Counter,
    pub query_duration: Histogram,
    pub candidates_considered: Histogram,
    pub vector_searches: Counter,
}

impl QueryMetrics {
    fn new() -> Self {
        Self {
            queries_total: counter("grakno.query.total", "Total queries executed"),
            query_duration: histogram(
                "grakno.query.duration_seconds",
                "Query pipeline latency",
                &[0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0],
            ),
            candidates_considered: histogram(
                "grakno.query.candidates_considered",
                "Number of candidates per query",
                &[1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0],
            ),
            vector_searches: counter(
                "grakno.query.vector_searches",
                "Queries using vector search",
            ),
        }
    }
}

impl Default for QueryMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Global metrics for store operations.
pub struct StoreMetrics {
    pub entities_total: Gauge,
    pub edges_total: Gauge,
    pub summaries_total: Gauge,
    pub embeddings_total: Gauge,
}

impl StoreMetrics {
    fn new() -> Self {
        Self {
            entities_total: gauge("grakno.store.entities", "Total entities in graph"),
            edges_total: gauge("grakno.store.edges", "Total edges in graph"),
            summaries_total: gauge("grakno.store.summaries", "Total summaries stored"),
            embeddings_total: gauge("grakno.store.embeddings", "Total embeddings stored"),
        }
    }
}

impl Default for StoreMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// Static instances for global access
static INDEX_METRICS: OnceLock<IndexMetrics> = OnceLock::new();
static QUERY_METRICS: OnceLock<QueryMetrics> = OnceLock::new();
static STORE_METRICS: OnceLock<StoreMetrics> = OnceLock::new();

/// Initialize all metric instances.
pub fn init_metrics() {
    let _ = INDEX_METRICS.set(IndexMetrics::new());
    let _ = QUERY_METRICS.set(QueryMetrics::new());
    let _ = STORE_METRICS.set(StoreMetrics::new());
}

/// Access index metrics.
pub fn index_metrics() -> &'static IndexMetrics {
    INDEX_METRICS.get().expect("metrics not initialized")
}

/// Access query metrics.
pub fn query_metrics() -> &'static QueryMetrics {
    QUERY_METRICS.get().expect("metrics not initialized")
}

/// Access store metrics.
pub fn store_metrics() -> &'static StoreMetrics {
    STORE_METRICS.get().expect("metrics not initialized")
}

/// Configuration for observability.
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    pub service_name: String,
    pub environment: String,
    pub otlp_endpoint: Option<String>,
    pub log_format: LogFormat,
    pub sampling_rate: Option<f64>,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            service_name: "grakno".into(),
            environment: "development".into(),
            otlp_endpoint: None,
            log_format: LogFormat::Pretty,
            sampling_rate: None,
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable pretty printing.
    Pretty,
    /// Structured JSON output.
    Json,
}

impl std::str::FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pretty" => Ok(LogFormat::Pretty),
            "json" => Ok(LogFormat::Json),
            _ => Err(format!("unknown log format: {s}")),
        }
    }
}

/// Initialize the observability stack with rolly.
///
/// Returns a guard that flushes pending telemetry on drop.
pub fn init_observability(config: &ObservabilityConfig) -> rolly::TelemetryGuard {
    init_metrics();

    let endpoint = config.otlp_endpoint.clone();

    rolly::init(rolly::TelemetryConfig {
        service_name: config.service_name.clone(),
        service_version: env!("CARGO_PKG_VERSION").into(),
        environment: config.environment.clone(),
        resource_attributes: vec![],
        otlp_traces_endpoint: endpoint.clone(),
        otlp_logs_endpoint: endpoint.clone(),
        otlp_metrics_endpoint: endpoint,
        log_to_stderr: config.log_format == LogFormat::Pretty,
        use_metrics_interval: None,
        metrics_flush_interval: Some(Duration::from_secs(10)),
        sampling_rate: config.sampling_rate,
        backpressure_strategy: rolly::BackpressureStrategy::Drop,
    })
}

/// Update store gauges from current stats.
pub fn record_store_stats(stats: &grakno_core::GraphStats) {
    let m = store_metrics();
    m.entities_total.set(stats.entities as f64, &[]);
    m.edges_total.set(stats.edges as f64, &[]);
    m.summaries_total.set(stats.summaries as f64, &[]);
    m.embeddings_total.set(stats.embeddings as f64, &[]);
}
