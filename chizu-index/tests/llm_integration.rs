use chizu_core::{
    ChizuStore, Config, Provider, ProviderError, Store,
};
use chizu_index::IndexPipeline;
use tempfile::TempDir;

struct MockProvider {
    summary_response: String,
    vectors: Vec<Vec<f32>>,
}

impl Provider for MockProvider {
    fn complete(&self, _prompt: &str) -> std::result::Result<String, ProviderError> {
        Ok(self.summary_response.clone())
    }

    fn embed(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, ProviderError> {
        let mut result = Vec::with_capacity(texts.len());
        for i in 0..texts.len() {
            result.push(self.vectors[i % self.vectors.len()].clone());
        }
        Ok(result)
    }
}

#[test]
fn index_pipeline_with_llm_populates_summaries_and_embeddings() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "test-crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    let mut config = Config::default();
    config.summary.provider = Some("ollama".to_string());
    config.summary.model = Some("test-model".to_string());
    config.embedding.provider = Some("ollama".to_string());
    config.embedding.model = Some("test-embed".to_string());
    config.embedding.dimensions = Some(4);
    config.embedding.batch_size = Some(8);

    let provider = MockProvider {
        summary_response: r#"{"short_summary": "Adds two numbers", "detailed_summary": "A simple addition function.", "keywords": ["math", "addition"]}"#.to_string(),
        vectors: vec![vec![1.0, 0.0, 0.0, 0.0]],
    };

    let store = ChizuStore::open(&temp_dir.path().join(".chizu"), &config).unwrap();
    let stats = IndexPipeline::run(root.as_path(), &store, &config, Some(&provider)).unwrap();

    assert!(stats.entities_inserted > 0);
    assert!(stats.summaries_generated > 0);
    assert!(stats.embeddings_generated > 0);

    let summaries = store.get_all_summaries().unwrap();
    assert!(!summaries.is_empty());
    let add_summary = summaries
        .iter()
        .find(|s| s.entity_id.contains("::add"))
        .expect("summary for add function should exist");
    assert_eq!(add_summary.short_summary, "Adds two numbers");
    assert_eq!(add_summary.detailed_summary, Some("A simple addition function.".to_string()));
    assert_eq!(add_summary.keywords, Some(vec!["math".to_string(), "addition".to_string()]));
    assert!(add_summary.source_hash.is_some());

    // Verify embeddings table
    let meta = store.get_embedding_meta(&add_summary.entity_id).unwrap().unwrap();
    assert_eq!(meta.model, "test-embed");
    assert_eq!(meta.dimensions, 4);
    assert!(meta.usearch_key.is_some());

    // Verify vector search resolves back to the correct entity.
    let results = store.search_vectors(&[1.0, 0.0, 0.0, 0.0], 5).unwrap();
    assert!(!results.is_empty());
    let found_key = results[0].0;
    let expected_key = meta.usearch_key.unwrap();
    assert_eq!(found_key, expected_key, "vector search should return the key for the 'add' entity");

    store.close().unwrap();
}

#[test]
fn index_pipeline_skips_llm_when_not_configured() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn foo() {}\n").unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "test-crate"
version = "0.1.0"
"#,
    )
    .unwrap();

    let config = Config::default();
    let store = ChizuStore::open(&temp_dir.path().join(".chizu"), &config).unwrap();
    let stats = IndexPipeline::run(root.as_path(), &store, &config, None).unwrap();

    assert!(stats.entities_inserted > 0);
    assert_eq!(stats.summaries_generated, 0);
    assert_eq!(stats.embeddings_generated, 0);

    let summaries = store.get_all_summaries().unwrap();
    assert!(summaries.is_empty());

    store.close().unwrap();
}
