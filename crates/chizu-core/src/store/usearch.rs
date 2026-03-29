use std::path::Path;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use super::{Result, StoreError};

/// Wrapper around usearch HNSW index.
pub struct UsearchIndex {
    index: Index,
    dimensions: usize,
    path: std::path::PathBuf,
}

impl UsearchIndex {
    /// Open an existing index or create a new one.
    pub fn open_or_create(path: &Path, dimensions: usize) -> Result<Self> {
        if path.exists() {
            Self::open(path, dimensions)
        } else {
            Self::create(path, dimensions)
        }
    }

    /// Create a new index.
    pub fn create(path: &Path, dimensions: usize) -> Result<Self> {
        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos, // Cosine similarity
            quantization: ScalarKind::F32,
            connectivity: 16,     // M in HNSW paper
            expansion_add: 128,   // ef_construction
            expansion_search: 64, // ef_search
            multi: false,
        };

        let index = Index::new(&options)
            .map_err(|e| StoreError::Usearch(format!("failed to create index: {:?}", e)))?;

        // Reserve capacity
        index
            .reserve(10000)
            .map_err(|e| StoreError::Usearch(format!("failed to reserve: {:?}", e)))?;

        Ok(Self {
            index,
            dimensions,
            path: path.to_path_buf(),
        })
    }

    /// Open an existing index.
    pub fn open(path: &Path, dimensions: usize) -> Result<Self> {
        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };

        let index = Index::new(&options)
            .map_err(|e| StoreError::Usearch(format!("failed to create index: {:?}", e)))?;

        index
            .load(
                path.to_str()
                    .ok_or_else(|| StoreError::Other("invalid path".into()))?,
            )
            .map_err(|e| StoreError::Usearch(format!("failed to load index: {:?}", e)))?;

        Ok(Self {
            index,
            dimensions,
            path: path.to_path_buf(),
        })
    }

    /// Get the number of vectors in the index.
    pub fn len(&self) -> usize {
        self.index.size()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get dimensions.
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Add a vector to the index.
    ///
    /// # Arguments
    /// * `key` - The usearch key (blake3 hash of entity_id as i64)
    /// * `vector` - The embedding vector
    pub fn add(&self, key: i64, vector: &[f32]) -> Result<()> {
        if vector.len() != self.dimensions {
            return Err(StoreError::Usearch(format!(
                "vector dimension mismatch: expected {}, got {}",
                self.dimensions,
                vector.len()
            )));
        }

        self.index
            .add(key as u64, vector)
            .map_err(|e| StoreError::Usearch(format!("failed to add vector: {:?}", e)))?;

        Ok(())
    }

    /// Search for nearest neighbors.
    ///
    /// # Arguments
    /// * `query` - The query vector
    /// * `k` - Number of results to return
    ///
    /// # Returns
    /// Vector of (key, distance) pairs, sorted by distance (ascending).
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
        if query.len() != self.dimensions {
            return Err(StoreError::Usearch(format!(
                "query dimension mismatch: expected {}, got {}",
                self.dimensions,
                query.len()
            )));
        }

        let results = self
            .index
            .search(query, k)
            .map_err(|e| StoreError::Usearch(format!("search failed: {:?}", e)))?;

        Ok(results
            .keys
            .iter()
            .zip(results.distances.iter())
            .map(|(&k, &d)| (k as i64, d))
            .collect())
    }

    /// Remove a vector by key.
    pub fn remove(&self, key: i64) -> Result<()> {
        self.index
            .remove(key as u64)
            .map_err(|e| StoreError::Usearch(format!("failed to remove vector: {:?}", e)))?;
        Ok(())
    }

    /// Check if a key exists.
    pub fn contains(&self, key: i64) -> bool {
        self.index.contains(key as u64)
    }

    /// Get a vector by key.
    /// Returns the vector if found.
    pub fn get(&self, key: i64) -> Result<Option<Vec<f32>>> {
        // Check if key exists first
        if !self.contains(key) {
            return Ok(None);
        }

        let mut buf = vec![0.0f32; self.dimensions];
        match self.index.get(key as u64, &mut buf) {
            Ok(_) => Ok(Some(buf)),
            Err(_) => Ok(None),
        }
    }

    /// Save the index to disk.
    pub fn save(&self) -> Result<()> {
        self.index
            .save(
                self.path
                    .to_str()
                    .ok_or_else(|| StoreError::Other("invalid path".into()))?,
            )
            .map_err(|e| StoreError::Usearch(format!("failed to save index: {:?}", e)))?;
        Ok(())
    }

    /// Close the index and save changes.
    pub fn close(&self) -> Result<()> {
        self.save()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_index(dimensions: usize) -> (UsearchIndex, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("test.usearch");
        let index = UsearchIndex::create(&index_path, dimensions).unwrap();
        (index, temp_dir)
    }

    #[test]
    fn test_create_index() {
        let (index, _temp) = create_test_index(128);
        assert_eq!(index.dimensions(), 128);
        assert!(index.is_empty());
    }

    #[test]
    fn test_add_and_search() {
        let (index, _temp) = create_test_index(4);

        // Add some vectors
        let v1 = vec![1.0, 0.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0, 0.0];
        let v3 = vec![0.0, 0.0, 1.0, 0.0];

        index.add(1, &v1).unwrap();
        index.add(2, &v2).unwrap();
        index.add(3, &v3).unwrap();

        assert_eq!(index.len(), 3);

        // Search for similar vectors
        let query = vec![0.9, 0.1, 0.0, 0.0];
        let results = index.search(&query, 2).unwrap();

        assert_eq!(results.len(), 2);
        // First result should be v1 (key 1) as it's most similar
        assert_eq!(results[0].0, 1);
    }

    #[test]
    fn test_remove() {
        let (index, _temp) = create_test_index(4);

        index.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        index.add(2, &[0.0, 1.0, 0.0, 0.0]).unwrap();

        assert!(index.contains(1));
        assert_eq!(index.len(), 2);

        index.remove(1).unwrap();

        assert!(!index.contains(1));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("test.usearch");

        // Create and populate
        {
            let index = UsearchIndex::create(&index_path, 4).unwrap();
            index.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
            index.add(2, &[0.0, 1.0, 0.0, 0.0]).unwrap();
            index.save().unwrap();
        }

        // Load and verify
        {
            let index = UsearchIndex::open(&index_path, 4).unwrap();
            assert_eq!(index.len(), 2);
            assert!(index.contains(1));
            assert!(index.contains(2));
        }
    }

    #[test]
    fn test_open_or_create() {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("test.usearch");

        // First call creates
        let index = UsearchIndex::open_or_create(&index_path, 4).unwrap();
        index.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        index.save().unwrap();
        drop(index);

        // Second call opens
        let index = UsearchIndex::open_or_create(&index_path, 4).unwrap();
        assert!(index.contains(1));
    }

    #[test]
    fn test_dimension_mismatch() {
        let (index, _temp) = create_test_index(4);

        // Try to add wrong dimension
        let wrong_vec = vec![1.0, 0.0, 0.0]; // 3 dimensions instead of 4
        let result = index.add(1, &wrong_vec);
        assert!(result.is_err());
    }

    #[test]
    fn test_search_returns_sorted_results() {
        let (index, _temp) = create_test_index(3);

        // Add vectors at different distances from origin
        index.add(1, &[1.0, 0.0, 0.0]).unwrap(); // distance from [0.9, 0.1, 0.0]: ~0.14
        index.add(2, &[0.0, 1.0, 0.0]).unwrap(); // distance: ~1.28
        index.add(3, &[0.0, 0.0, 1.0]).unwrap(); // distance: ~1.81

        let query = vec![0.9, 0.1, 0.0];
        let results = index.search(&query, 3).unwrap();

        // Results should be sorted by distance (ascending)
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 1); // Closest

        // Verify distances are ascending
        for i in 1..results.len() {
            assert!(results[i - 1].1 <= results[i].1);
        }
    }

    #[test]
    fn test_get_vector() {
        let (index, _temp) = create_test_index(4);

        let original = vec![1.0, 2.0, 3.0, 4.0];
        index.add(1, &original).unwrap();

        let retrieved = index.get(1).unwrap();
        assert!(retrieved.is_some());

        let vec = retrieved.unwrap();
        assert_eq!(vec.len(), 4);
        assert!((vec[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_get_nonexistent() {
        let (index, _temp) = create_test_index(4);

        let result = index.get(999);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
