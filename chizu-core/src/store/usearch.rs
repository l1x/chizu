use std::path::Path;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

use super::{Result, StoreError};

/// Extension trait to convert any `Result<T, E: Debug>` into our `Result<T>`.
trait MapUsearchErr<T> {
    fn usearch(self, context: &str) -> Result<T>;
}

impl<T, E: std::fmt::Debug> MapUsearchErr<T> for std::result::Result<T, E> {
    fn usearch(self, context: &str) -> Result<T> {
        self.map_err(|e| StoreError::Usearch(format!("{context}: {e:?}")))
    }
}

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

    pub fn create(path: &Path, dimensions: usize) -> Result<Self> {
        let index = Index::new(&Self::index_options(dimensions)).usearch("create index")?;
        index.reserve(10000).usearch("reserve")?;

        Ok(Self {
            index,
            dimensions,
            path: path.to_path_buf(),
        })
    }

    pub fn open(path: &Path, dimensions: usize) -> Result<Self> {
        let index = Index::new(&Self::index_options(dimensions)).usearch("create index")?;
        let path_str = path
            .to_str()
            .ok_or_else(|| StoreError::Other("invalid path".into()))?;
        index.load(path_str).usearch("load index")?;

        Ok(Self {
            index,
            dimensions,
            path: path.to_path_buf(),
        })
    }

    fn index_options(dimensions: usize) -> IndexOptions {
        IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::F32,
            connectivity: 16,     // M in HNSW paper
            expansion_add: 128,   // ef_construction
            expansion_search: 64, // ef_search
            multi: false,
        }
    }

    pub fn len(&self) -> usize {
        self.index.size()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Add a vector to the index.
    ///
    /// Keys are 64-bit blake3 hash truncations. Birthday collision probability:
    /// ~n²/2^65 — at 1M entities ≈ 5.4×10⁻⁸ (negligible). Collision detection
    /// is handled at the [`ChizuStore`] level by cross-referencing the
    /// embeddings table before insert.
    pub fn add(&self, key: i64, vector: &[f32]) -> Result<()> {
        if vector.len() != self.dimensions {
            return Err(StoreError::Usearch(format!(
                "vector dimension mismatch: expected {}, got {}",
                self.dimensions,
                vector.len()
            )));
        }
        self.index.add(key as u64, vector).usearch("add vector")?;
        Ok(())
    }

    /// Returns `(key, distance)` pairs sorted by ascending distance.
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<(i64, f32)>> {
        if query.len() != self.dimensions {
            return Err(StoreError::Usearch(format!(
                "query dimension mismatch: expected {}, got {}",
                self.dimensions,
                query.len()
            )));
        }
        let results = self.index.search(query, k).usearch("search")?;
        Ok(results
            .keys
            .iter()
            .zip(results.distances.iter())
            .map(|(&k, &d)| (k as i64, d))
            .collect())
    }

    pub fn remove(&self, key: i64) -> Result<()> {
        self.index.remove(key as u64).usearch("remove vector")?;
        Ok(())
    }

    pub fn contains(&self, key: i64) -> bool {
        self.index.contains(key as u64)
    }

    /// Returns `None` if the key is not in the index.
    pub fn get(&self, key: i64) -> Result<Option<Vec<f32>>> {
        if !self.index.contains(key as u64) {
            return Ok(None);
        }
        let mut buf = vec![0.0f32; self.dimensions];
        match self.index.get(key as u64, &mut buf) {
            Ok(_) => Ok(Some(buf)),
            Err(_) => Ok(None),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path_str = self
            .path
            .to_str()
            .ok_or_else(|| StoreError::Other("invalid path".into()))?;
        self.index.save(path_str).usearch("save index")?;
        Ok(())
    }

    pub fn close(&self) -> Result<()> {
        self.save()
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

        index.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        index.add(2, &[0.0, 1.0, 0.0, 0.0]).unwrap();
        index.add(3, &[0.0, 0.0, 1.0, 0.0]).unwrap();
        assert_eq!(index.len(), 3);

        let results = index.search(&[0.9, 0.1, 0.0, 0.0], 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 1);
    }

    #[test]
    fn test_remove() {
        let (index, _temp) = create_test_index(4);

        index.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        index.add(2, &[0.0, 1.0, 0.0, 0.0]).unwrap();
        assert_eq!(index.len(), 2);

        index.remove(1).unwrap();
        assert!(!index.contains(1));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("test.usearch");

        {
            let index = UsearchIndex::create(&index_path, 4).unwrap();
            index.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
            index.add(2, &[0.0, 1.0, 0.0, 0.0]).unwrap();
            index.save().unwrap();
        }

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

        let index = UsearchIndex::open_or_create(&index_path, 4).unwrap();
        index.add(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        index.save().unwrap();
        drop(index);

        let index = UsearchIndex::open_or_create(&index_path, 4).unwrap();
        assert!(index.contains(1));
    }

    #[test]
    fn test_dimension_mismatch() {
        let (index, _temp) = create_test_index(4);
        assert!(index.add(1, &[1.0, 0.0, 0.0]).is_err());
    }

    #[test]
    fn test_search_returns_sorted_results() {
        let (index, _temp) = create_test_index(3);

        index.add(1, &[1.0, 0.0, 0.0]).unwrap();
        index.add(2, &[0.0, 1.0, 0.0]).unwrap();
        index.add(3, &[0.0, 0.0, 1.0]).unwrap();

        let results = index.search(&[0.9, 0.1, 0.0], 3).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 1);
        for i in 1..results.len() {
            assert!(results[i - 1].1 <= results[i].1);
        }
    }

    #[test]
    fn test_get_vector() {
        let (index, _temp) = create_test_index(4);
        index.add(1, &[1.0, 2.0, 3.0, 4.0]).unwrap();

        let vec = index.get(1).unwrap().unwrap();
        assert_eq!(vec.len(), 4);
        assert!((vec[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_get_nonexistent() {
        let (index, _temp) = create_test_index(4);
        assert!(index.get(999).unwrap().is_none());
    }
}
