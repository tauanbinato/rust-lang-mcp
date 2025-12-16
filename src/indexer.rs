use std::path::Path;

use crate::error::Result;
use crate::parsing::{parse_markdown_file, Document};
use crate::search::embeddings::{embed_texts, init_embedding_model};
use crate::search::{SearchIndex, VectorIndex};
use crate::sources::{DocSource, DOC_SOURCES};

/// Index all available documentation sources (keyword index only)
pub fn index_all_sources(index: &SearchIndex, data_dir: &Path) -> Result<usize> {
    let all_documents = collect_all_documents(data_dir)?;

    if all_documents.is_empty() {
        tracing::warn!("No documents found to index");
        return Ok(0);
    }

    let count = all_documents.len();
    tracing::info!("Indexing {} total documents", count);
    index.index_documents(&all_documents)?;

    Ok(count)
}

/// Index all sources with both keyword and vector indices (hybrid search)
#[allow(dead_code)]
pub fn index_all_sources_hybrid(
    keyword_index: &SearchIndex,
    vector_index: &mut VectorIndex,
    data_dir: &Path,
) -> Result<usize> {
    let all_documents = collect_all_documents(data_dir)?;

    if all_documents.is_empty() {
        tracing::warn!("No documents found to index");
        return Ok(0);
    }

    let count = all_documents.len();
    tracing::info!("Indexing {} total documents with hybrid search", count);

    // Index in keyword search
    keyword_index.index_documents(&all_documents)?;

    // Clear and rebuild vector index
    vector_index.clear();

    // Initialize embedding model
    let models_dir = data_dir.join("models");
    init_embedding_model(&models_dir)?;

    // Generate embeddings in batches
    const BATCH_SIZE: usize = 32;
    let mut indexed = 0;

    for chunk in all_documents.chunks(BATCH_SIZE) {
        // Prepare texts for embedding (use content or title if content is too short)
        let texts: Vec<&str> = chunk
            .iter()
            .map(|doc| {
                if doc.content.len() > 50 {
                    doc.content.as_str()
                } else {
                    // For short content, combine title and content
                    doc.title.as_str()
                }
            })
            .collect();

        // Generate embeddings
        match embed_texts(&texts) {
            Ok(embeddings) => {
                for (doc, embedding) in chunk.iter().zip(embeddings.into_iter()) {
                    vector_index.add(doc.path.clone(), embedding);
                }
                indexed += chunk.len();
                tracing::debug!("Embedded {}/{} documents", indexed, count);
            }
            Err(e) => {
                tracing::warn!("Failed to generate embeddings for batch: {}", e);
            }
        }
    }

    tracing::info!("Indexed {} documents with {} embeddings", count, indexed);

    // Save vector index
    let vector_index_path = data_dir.join("index").join("vectors");
    vector_index.save(&vector_index_path)?;

    Ok(count)
}

/// Collect all documents from all sources
fn collect_all_documents(data_dir: &Path) -> Result<Vec<Document>> {
    let mut all_documents = Vec::new();

    for source in DOC_SOURCES {
        let docs_path = source.docs_path(data_dir);
        if docs_path.exists() {
            tracing::info!("Collecting documents from {} ({:?})", source.name, docs_path);
            match collect_documents(&docs_path, source.id) {
                Ok(docs) => {
                    tracing::info!("  Found {} documents", docs.len());
                    all_documents.extend(docs);
                }
                Err(e) => {
                    tracing::warn!("  Failed to collect from {}: {}", source.id, e);
                }
            }
        } else {
            tracing::debug!("Source {} not available at {:?}", source.id, docs_path);
        }
    }

    Ok(all_documents)
}

/// Index a single documentation source
#[allow(dead_code)]
pub fn index_source(index: &SearchIndex, data_dir: &Path, source: &DocSource) -> Result<usize> {
    let docs_path = source.docs_path(data_dir);
    let documents = collect_documents(&docs_path, source.id)?;

    if documents.is_empty() {
        tracing::warn!("No markdown files found in {:?}", docs_path);
        return Ok(0);
    }

    let count = documents.len();
    tracing::info!("Indexing {} documents from {}", count, source.name);
    index.index_documents(&documents)?;

    Ok(count)
}

/// Recursively collect all markdown documents from a directory
fn collect_documents(dir: &Path, source: &str) -> Result<Vec<Document>> {
    let mut documents = Vec::new();

    if !dir.exists() {
        return Ok(documents);
    }

    for entry in walkdir(dir)? {
        let path = entry;
        if path.extension().is_some_and(|ext| ext == "md") {
            match parse_markdown_file(&path, source) {
                Ok(doc) => documents.push(doc),
                Err(e) => {
                    tracing::warn!("Failed to parse {:?}: {}", path, e);
                }
            }
        }
    }

    Ok(documents)
}

/// Simple recursive directory walker
fn walkdir(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            files.extend(walkdir(&path)?);
        } else {
            files.push(path);
        }
    }

    Ok(files)
}
