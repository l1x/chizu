use chizu_core::{
    ChizuStore, EmbeddingConfig, EmbeddingMeta, Provider, Store, entity_id_to_usearch_key,
};
use tracing::{debug, error, warn};

use crate::error::Result;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddingStats {
    pub generated: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub struct Embedder<'a> {
    provider: &'a dyn Provider,
    config: &'a EmbeddingConfig,
}

impl<'a> Embedder<'a> {
    pub fn new(provider: &'a dyn Provider, config: &'a EmbeddingConfig) -> Self {
        Self { provider, config }
    }

    pub fn run(&self, store: &ChizuStore) -> Result<EmbeddingStats> {
        let mut stats = EmbeddingStats::default();

        let Some(ref model) = self.config.model else {
            debug!("No embedding model configured; skipping");
            return Ok(stats);
        };

        let summaries = store.get_all_summaries()?;
        if summaries.is_empty() {
            debug!("No summaries to embed");
            return Ok(stats);
        }

        let batch_size = self.config.batch_size.unwrap_or(32).max(1);
        let dimensions = self.config.dimensions.unwrap_or(768);

        let mut batch: Vec<(String, String)> = Vec::with_capacity(batch_size);

        for summary in summaries {
            // Skip if an embedding for this model already exists.
            if let Some(existing) = store.get_embedding_meta(&summary.entity_id)? {
                if existing.model == *model {
                    stats.skipped += 1;
                    continue;
                }
            }

            let entity = match store.get_entity(&summary.entity_id)? {
                Some(e) => e,
                None => {
                    warn!(
                        "Entity {} not found for embedding; skipping",
                        summary.entity_id
                    );
                    continue;
                }
            };

            let text = build_embedding_text(&entity, &summary);
            batch.push((entity.id, text));

            if batch.len() >= batch_size {
                self.flush_batch(store, model, dimensions, &batch, &mut stats);
                batch.clear();
            }
        }

        if !batch.is_empty() {
            self.flush_batch(store, model, dimensions, &batch, &mut stats);
        }

        Ok(stats)
    }

    /// Try to embed a batch; on failure, fall back to embedding each item individually.
    fn flush_batch(
        &self,
        store: &ChizuStore,
        model: &str,
        dimensions: u32,
        batch: &[(String, String)],
        stats: &mut EmbeddingStats,
    ) {
        if let Err(e) = self.process_batch(store, model, dimensions, batch) {
            error!("Batch embedding failed: {e}; falling back to singles");
            for (id, text) in batch {
                if let Err(e) = self.process_single(store, model, dimensions, id, text) {
                    error!("Single embedding failed for {id}: {e}");
                    stats.failed += 1;
                } else {
                    stats.generated += 1;
                }
            }
        } else {
            stats.generated += batch.len();
        }
    }

    fn process_batch(
        &self,
        store: &ChizuStore,
        model: &str,
        dimensions: u32,
        batch: &[(String, String)],
    ) -> Result<()> {
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let vectors = self.provider.embed(&texts)?;

        if vectors.len() != batch.len() {
            return Err(crate::error::IndexError::Other(format!(
                "embedding count mismatch: expected {}, got {}",
                batch.len(),
                vectors.len()
            )));
        }

        for (i, v) in vectors.iter().enumerate() {
            if v.len() != dimensions as usize {
                return Err(crate::error::IndexError::Other(format!(
                    "embedding dimension mismatch for item {}: expected {}, got {}",
                    i, dimensions, v.len()
                )));
            }
        }

        // Compute keys upfront so we can write metadata first.
        let keyed: Vec<_> = batch
            .iter()
            .zip(vectors.iter())
            .map(|((entity_id, _), vector)| {
                let key = entity_id_to_usearch_key(entity_id);
                (entity_id.as_str(), key, vector)
            })
            .collect();

        // Write SQLite metadata in a transaction first, then add vectors.
        // This ensures metadata is never behind the vector index: if a
        // vector write fails after the transaction commits, the metadata
        // still points to the correct key and re-indexing will overwrite.
        store
            .in_transaction(|store| {
                for &(entity_id, key, _) in &keyed {
                    let meta =
                        EmbeddingMeta::new(entity_id, model, dimensions).with_usearch_key(key);
                    store.insert_embedding_meta(&meta)?;
                }
                Ok(())
            })
            .map_err(crate::error::IndexError::Store)?;

        for &(entity_id, key, vector) in &keyed {
            store.add_vector(entity_id, key, vector)?;
        }

        Ok(())
    }

    fn process_single(
        &self,
        store: &ChizuStore,
        model: &str,
        dimensions: u32,
        entity_id: &str,
        text: &str,
    ) -> Result<()> {
        let vectors = self.provider.embed(&[text.to_string()])?;

        let vector = vectors.into_iter().next().ok_or_else(|| {
            crate::error::IndexError::Other("empty embedding response".into())
        })?;

        let key = entity_id_to_usearch_key(entity_id);
        let meta = EmbeddingMeta::new(entity_id, model, dimensions).with_usearch_key(key);
        store.insert_embedding_meta(&meta)?;
        store.add_vector(entity_id, key, &vector)?;

        Ok(())
    }
}

fn build_embedding_text(entity: &chizu_core::Entity, summary: &chizu_core::Summary) -> String {
    let mut parts = vec![format!("{}: {}", entity.kind, entity.name)];
    if !summary.short_summary.is_empty() {
        parts.push(summary.short_summary.clone());
    }
    if let Some(ref keywords) = summary.keywords {
        if !keywords.is_empty() {
            parts.push(format!("Keywords: {}", keywords.join(", ")));
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chizu_core::{
        ChizuStore, Config, Entity, EntityKind, Provider, ProviderError, Store, Summary,
    };
    use tempfile::TempDir;

    struct MockProvider {
        vectors: Vec<Vec<f32>>,
    }

    impl Provider for MockProvider {
        fn complete(&self, _prompt: &str, _max_tokens: Option<u32>) -> std::result::Result<String, ProviderError> {
            unimplemented!()
        }

        fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
            Ok((0..texts.len())
                .map(|i| self.vectors[i % self.vectors.len()].clone())
                .collect())
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
    fn test_build_embedding_text() {
        let entity = Entity::new("s1", EntityKind::Symbol, "foo");
        let summary = Summary::new("s1", "A function").with_keywords(&["rust", "test"]);
        let text = build_embedding_text(&entity, &summary);
        assert!(text.contains("symbol: foo"));
        assert!(text.contains("A function"));
        assert!(text.contains("Keywords: rust, test"));
    }

    #[test]
    fn test_embedder_generates_and_skips() {
        let (store, _temp) = create_test_store(4);

        store
            .insert_entity(&Entity::new(
                "symbol::src/lib.rs::foo",
                EntityKind::Symbol,
                "foo",
            ))
            .unwrap();
        store
            .insert_summary(&Summary::new("symbol::src/lib.rs::foo", "A function"))
            .unwrap();

        let provider = MockProvider {
            vectors: vec![vec![1.0, 0.0, 0.0, 0.0]],
        };
        let config = EmbeddingConfig {
            provider: Some("test".to_string()),
            model: Some("test-model".to_string()),
            dimensions: Some(4),
            batch_size: Some(2),
        };
        let embedder = Embedder::new(&provider, &config);

        let stats1 = embedder.run(&store).unwrap();
        assert_eq!(stats1.generated, 1);
        assert_eq!(stats1.skipped, 0);

        let meta = store
            .get_embedding_meta("symbol::src/lib.rs::foo")
            .unwrap()
            .unwrap();
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.dimensions, 4);
        assert!(meta.usearch_key.is_some());

        let key = entity_id_to_usearch_key("symbol::src/lib.rs::foo");
        let vector = store.get_vector(key).unwrap().unwrap();
        assert_eq!(vector, vec![1.0, 0.0, 0.0, 0.0]);

        let stats2 = embedder.run(&store).unwrap();
        assert_eq!(stats2.generated, 0);
        assert_eq!(stats2.skipped, 1);
    }

    #[test]
    fn test_embedder_search_returns_correct_entity() {
        let (store, _temp) = create_test_store(4);

        store
            .insert_entity(&Entity::new(
                "symbol::src/lib.rs::bar",
                EntityKind::Symbol,
                "bar",
            ))
            .unwrap();
        store
            .insert_summary(&Summary::new("symbol::src/lib.rs::bar", "A function"))
            .unwrap();

        let provider = MockProvider {
            vectors: vec![vec![1.0, 0.0, 0.0, 0.0]],
        };
        let config = EmbeddingConfig {
            provider: Some("test".to_string()),
            model: Some("test-model".to_string()),
            dimensions: Some(4),
            batch_size: Some(2),
        };
        Embedder::new(&provider, &config).run(&store).unwrap();

        let results = store.search_vectors(&[1.0, 0.0, 0.0, 0.0], 1).unwrap();
        assert_eq!(results.len(), 1);
        let key = entity_id_to_usearch_key("symbol::src/lib.rs::bar");
        assert_eq!(results[0].0, key);
    }
}
