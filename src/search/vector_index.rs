//! Vector index for semantic search using HNSW (Hierarchical Navigable Small World).

#![allow(dead_code)]

use std::path::Path;

use hnsw_rs::hnsw::Hnsw;
use hnsw_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Stored document with path and embedding for persistence
#[derive(Serialize, Deserialize)]
struct StoredDocument {
    path: String,
    embedding: Vec<f32>,
}

/// Vector index for storing and searching document embeddings
pub struct VectorIndex {
    hnsw: Hnsw<'static, f32, DistCosine>,
    /// Mapping from HNSW internal ID to document path
    id_to_path: Vec<String>,
    /// Store embeddings for persistence (rebuild index on load)
    embeddings: Vec<Vec<f32>>,
}

impl VectorIndex {
    /// Create a new empty vector index
    pub fn new() -> Self {
        let nb_elem = 10000; // Initial capacity
        let max_nb_connection = 16; // M parameter
        let nb_layer = 16; // Max layers
        let ef_construction = 200; // Build-time search width

        let hnsw = Hnsw::new(max_nb_connection, nb_elem, nb_layer, ef_construction, DistCosine);

        Self {
            hnsw,
            id_to_path: Vec::new(),
            embeddings: Vec::new(),
        }
    }

    /// Add a single document to the index
    pub fn add(&mut self, path: String, embedding: Vec<f32>) {
        let id = self.id_to_path.len();
        self.hnsw.insert((&embedding, id));
        self.id_to_path.push(path);
        self.embeddings.push(embedding);
    }

    /// Add multiple documents to the index
    pub fn add_batch(&mut self, documents: Vec<(String, Vec<f32>)>) {
        let start_id = self.id_to_path.len();

        // Prepare data for parallel insertion
        let data: Vec<(&Vec<f32>, usize)> = documents
            .iter()
            .enumerate()
            .map(|(i, (_, embedding))| (embedding, start_id + i))
            .collect();

        // Insert into HNSW index
        self.hnsw.parallel_insert(&data);

        // Store path mappings and embeddings
        for (path, embedding) in documents {
            self.id_to_path.push(path);
            self.embeddings.push(embedding);
        }
    }

    /// Search for similar documents
    pub fn search(&self, query_embedding: &[f32], limit: usize) -> Vec<(String, f32)> {
        let ef_search = limit.max(32); // Search width (higher = more accurate, slower)

        let neighbors = self.hnsw.search(query_embedding, limit, ef_search);

        neighbors
            .into_iter()
            .filter_map(|neighbor| {
                let idx = neighbor.d_id;
                if idx < self.id_to_path.len() {
                    // Convert distance to similarity score (cosine distance -> similarity)
                    let similarity = 1.0 - neighbor.distance;
                    Some((self.id_to_path[idx].clone(), similarity))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the number of documents in the index
    pub fn len(&self) -> usize {
        self.id_to_path.len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.id_to_path.is_empty()
    }

    /// Save the index to disk (stores documents as JSON, rebuilds HNSW on load)
    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)?;

        // Save documents (paths + embeddings) as JSON
        let documents: Vec<StoredDocument> = self
            .id_to_path
            .iter()
            .zip(self.embeddings.iter())
            .map(|(path, embedding)| StoredDocument {
                path: path.clone(),
                embedding: embedding.clone(),
            })
            .collect();

        let docs_path = path.join("vector_index.json");
        let file = std::fs::File::create(&docs_path)?;
        serde_json::to_writer(file, &documents)
            .map_err(|e| Error::Other(format!("Failed to save vector index: {}", e)))?;

        tracing::info!("Saved {} vectors to {:?}", documents.len(), docs_path);
        Ok(())
    }

    /// Load the index from disk (rebuilds HNSW from stored embeddings)
    pub fn load(path: &Path) -> Result<Self> {
        let docs_path = path.join("vector_index.json");

        if !docs_path.exists() {
            return Err(Error::Other("Vector index file not found".to_string()));
        }

        // Load documents
        let file = std::fs::File::open(&docs_path)?;
        let documents: Vec<StoredDocument> = serde_json::from_reader(file)
            .map_err(|e| Error::Other(format!("Failed to load vector index: {}", e)))?;

        tracing::info!("Loading {} vectors from {:?}", documents.len(), docs_path);

        // Create new index and rebuild HNSW
        let mut index = Self::new();
        for doc in documents {
            index.add(doc.path, doc.embedding);
        }

        Ok(index)
    }

    /// Load or create the index
    pub fn open_or_create(path: &Path) -> Result<Self> {
        let docs_path = path.join("vector_index.json");

        if docs_path.exists() {
            Self::load(path)
        } else {
            std::fs::create_dir_all(path)?;
            Ok(Self::new())
        }
    }

    /// Clear all documents from the index
    pub fn clear(&mut self) {
        self.hnsw = Hnsw::new(16, 10000, 16, 200, DistCosine);
        self.id_to_path.clear();
        self.embeddings.clear();
    }
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_index_basic() {
        let mut index = VectorIndex::new();

        // Add some documents
        index.add("doc1.md".to_string(), vec![1.0, 0.0, 0.0]);
        index.add("doc2.md".to_string(), vec![0.0, 1.0, 0.0]);
        index.add("doc3.md".to_string(), vec![0.0, 0.0, 1.0]);

        assert_eq!(index.len(), 3);

        // Search for similar to doc1
        let results = index.search(&[1.0, 0.0, 0.0], 2);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "doc1.md");
    }

    #[test]
    fn test_vector_index_empty() {
        let index = VectorIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }
}
