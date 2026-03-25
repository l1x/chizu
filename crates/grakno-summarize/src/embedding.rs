use std::fmt;
use std::thread;
use std::time::Duration;

use grakno_core::model::{EmbeddingRecord, Summary};
use grakno_core::Store;
use serde::{Deserialize, Serialize};

use crate::config::SummarizeConfig;
use crate::error::{Result, SummarizeError};

// ---------------------------------------------------------------------------
// Embedding client
// ---------------------------------------------------------------------------

pub struct EmbeddingClient {
    http: reqwest::blocking::Client,
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF_SECS: u64 = 2;
const BATCH_SIZE: usize = 64;

impl EmbeddingClient {
    pub fn new(config: &SummarizeConfig) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
        })
    }

    /// Call the OpenAI-compatible /embeddings endpoint.
    /// Returns one embedding vector per input text, in input order.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = EmbeddingRequest {
            model: self.model.clone(),
            input: texts.iter().map(|s| s.to_string()).collect(),
        };

        let mut last_err: Option<SummarizeError> = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = Duration::from_secs(BASE_BACKOFF_SECS * u64::from(attempt));
                thread::sleep(backoff);
            }

            let mut req = self.http.post(&url).json(&body);
            if !self.api_key.is_empty() {
                req = req.bearer_auth(&self.api_key);
            }

            let response = match req.send() {
                Ok(r) => r,
                Err(e) => return Err(SummarizeError::Http(e)),
            };

            let status = response.status().as_u16();
            if status == 200 {
                let mut resp: EmbeddingResponse = response.json()?;
                // Sort by index to guarantee input order
                resp.data.sort_by_key(|d| d.index);
                return Ok(resp.data.into_iter().map(|d| d.embedding).collect());
            }

            let body_text = response.text().unwrap_or_default();

            if status == 429 || status >= 500 {
                last_err = Some(SummarizeError::Api {
                    status,
                    body: body_text,
                });
                continue;
            }

            return Err(SummarizeError::Api {
                status,
                body: body_text,
            });
        }

        Err(last_err.unwrap_or_else(|| SummarizeError::Api {
            status: 0,
            body: "max retries exceeded".to_string(),
        }))
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

// ---------------------------------------------------------------------------
// embed_graph
// ---------------------------------------------------------------------------

pub struct EmbedOptions {
    pub component: Option<String>,
    pub force: bool,
}

pub struct EmbedStats {
    pub entities_processed: usize,
    pub entities_skipped: usize,
    pub entities_embedded: usize,
    pub errors: usize,
}

impl fmt::Display for EmbedStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "processed: {}, embedded: {}, skipped: {}, errors: {}",
            self.entities_processed, self.entities_embedded, self.entities_skipped, self.errors,
        )
    }
}

pub fn embed_graph(
    store: &Store,
    client: &EmbeddingClient,
    options: &EmbedOptions,
) -> Result<EmbedStats> {
    let mut stats = EmbedStats {
        entities_processed: 0,
        entities_skipped: 0,
        entities_embedded: 0,
        errors: 0,
    };

    // Gather entities that have summaries
    let entities = match &options.component {
        Some(comp_id) => store.list_entities_by_component(comp_id)?,
        None => store.list_entities()?,
    };

    // Collect (entity_id, summary_text) pairs that need embedding
    let mut to_embed: Vec<(String, String)> = Vec::new();

    for entity in &entities {
        let summary: Summary = match store.get_summary(&entity.id) {
            Ok(s) => s,
            Err(_) => continue, // no summary → skip
        };

        stats.entities_processed += 1;

        if !options.force {
            if let Ok(existing) = store.get_embedding(&entity.id) {
                // Skip if embedding is newer than or equal to the summary
                if existing.updated_at >= summary.updated_at {
                    stats.entities_skipped += 1;
                    continue;
                }
            }
        }

        to_embed.push((entity.id.clone(), summary.short_summary));
    }

    // Process in batches
    for batch in to_embed.chunks(BATCH_SIZE) {
        let texts: Vec<&str> = batch.iter().map(|(_, text)| text.as_str()).collect();

        eprint!("embedding {} entities... ", batch.len());

        match client.embed(&texts) {
            Ok(vectors) => {
                for (i, vec) in vectors.into_iter().enumerate() {
                    let (entity_id, _) = &batch[i];
                    let dimensions = vec.len() as i64;
                    let record = EmbeddingRecord {
                        entity_id: entity_id.clone(),
                        model: client.model().to_string(),
                        dimensions,
                        vector: vec,
                        updated_at: now_iso8601(),
                    };
                    match store.upsert_embedding(&record) {
                        Ok(()) => stats.entities_embedded += 1,
                        Err(e) => {
                            eprintln!("store error for {entity_id}: {e}");
                            stats.errors += 1;
                        }
                    }
                }
                eprintln!("done");
            }
            Err(e) => {
                eprintln!("error: {e}");
                stats.errors += batch.len();
            }
        }
    }

    Ok(stats)
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

pub struct SearchResult {
    pub entity_id: String,
    pub distance: f32,
    pub entity_name: String,
    pub entity_kind: String,
    pub path: Option<String>,
    pub line_start: Option<i64>,
    pub short_summary: String,
}

pub fn search(
    store: &Store,
    client: &EmbeddingClient,
    query: &str,
    k: usize,
) -> Result<Vec<SearchResult>> {
    // Embed the query
    let vectors = client.embed(&[query])?;
    let query_vec = vectors
        .into_iter()
        .next()
        .ok_or_else(|| SummarizeError::ParseResponse("empty embedding response".to_string()))?;

    // Vector search
    let hits = store.vector_search(&query_vec, k)?;

    // Enrich with entity + summary data
    let mut results = Vec::with_capacity(hits.len());
    for hit in hits {
        let entity = match store.get_entity(&hit.entity_id) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let summary_text = store
            .get_summary(&hit.entity_id)
            .map(|s| s.short_summary)
            .unwrap_or_default();

        results.push(SearchResult {
            entity_id: hit.entity_id,
            distance: hit.distance,
            entity_name: entity.name,
            entity_kind: entity.kind.to_string(),
            path: entity.path,
            line_start: entity.line_start,
            short_summary: summary_text,
        });
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Simple entity embedding (no summaries required)
// ---------------------------------------------------------------------------

pub struct SimpleEmbedOptions {
    pub force: bool,
}

/// Embed entities using their metadata (name, kind, path) instead of summaries.
/// This is useful for initial indexing when summaries haven't been generated yet.
pub fn embed_entities_simple(
    store: &Store,
    client: &EmbeddingClient,
    options: &SimpleEmbedOptions,
) -> Result<EmbedStats> {
    use grakno_core::model::Entity;
    
    let mut stats = EmbedStats {
        entities_processed: 0,
        entities_skipped: 0,
        entities_embedded: 0,
        errors: 0,
    };

    // Get all entities
    let entities: Vec<Entity> = store.list_entities()?;

    // Build (entity_id, text_to_embed) pairs
    let mut to_embed: Vec<(String, String)> = Vec::new();

    for entity in &entities {
        stats.entities_processed += 1;

        if !options.force {
            if store.get_embedding(&entity.id).is_ok() {
                stats.entities_skipped += 1;
                continue;
            }
        }

        // Create a simple text representation of the entity
        let text = format!(
            "{} {} {}",
            entity.kind,
            entity.name,
            entity.path.as_deref().unwrap_or("")
        );

        to_embed.push((entity.id.clone(), text));
    }

    // Process in batches
    for batch in to_embed.chunks(BATCH_SIZE) {
        let texts: Vec<&str> = batch.iter().map(|(_, text)| text.as_str()).collect();

        eprint!("embedding {} entities... ", batch.len());

        match client.embed(&texts) {
            Ok(vectors) => {
                for (i, vec) in vectors.into_iter().enumerate() {
                    let (entity_id, _) = &batch[i];
                    let dimensions = vec.len() as i64;
                    let record = EmbeddingRecord {
                        entity_id: entity_id.clone(),
                        model: client.model().to_string(),
                        dimensions,
                        vector: vec,
                        updated_at: now_iso8601(),
                    };
                    match store.upsert_embedding(&record) {
                        Ok(()) => stats.entities_embedded += 1,
                        Err(e) => {
                            eprintln!("store error for {entity_id}: {e}");
                            stats.errors += 1;
                        }
                    }
                }
                eprintln!("done");
            }
            Err(e) => {
                eprintln!("error: {e}");
                stats.errors += batch.len();
            }
        }
    }

    Ok(stats)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn now_iso8601() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
