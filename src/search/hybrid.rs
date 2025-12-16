//! Hybrid search combining keyword and semantic search with RRF score fusion.

use std::collections::HashMap;

use crate::error::Result;
use crate::search::embeddings::embed_text;
use crate::search::index::{SearchIndex, SearchResult};
use crate::search::vector_index::VectorIndex;

/// RRF constant (standard value from the original paper)
const RRF_K: f32 = 60.0;

/// Hybrid search engine combining keyword and semantic search
pub struct HybridSearch<'a> {
    keyword_index: &'a SearchIndex,
    vector_index: &'a VectorIndex,
}

impl<'a> HybridSearch<'a> {
    /// Create a new hybrid search engine
    pub fn new(keyword_index: &'a SearchIndex, vector_index: &'a VectorIndex) -> Self {
        Self {
            keyword_index,
            vector_index,
        }
    }

    /// Perform hybrid search combining keyword and semantic results
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search_with_sources(query, limit, None)
    }

    /// Perform hybrid search with optional source filtering
    pub fn search_with_sources(
        &self,
        query: &str,
        limit: usize,
        sources: Option<&[&str]>,
    ) -> Result<Vec<SearchResult>> {
        // Get more results from each method to ensure good coverage after fusion
        let expanded_limit = limit * 3;

        // Run keyword search (with source filtering)
        let keyword_results =
            self.keyword_index
                .search_with_sources(query, expanded_limit, sources)?;

        // Run semantic search
        let query_embedding = embed_text(query)?;
        let mut semantic_results = self.vector_index.search(&query_embedding, expanded_limit);

        // Filter semantic results by source if specified
        if let Some(sources) = sources {
            semantic_results.retain(|(path, _)| {
                // First check if it's in keyword results
                if keyword_results
                    .iter()
                    .any(|r| r.path == *path && sources.contains(&r.source.as_str()))
                {
                    return true;
                }

                // Otherwise, look up the source by querying for the exact path
                // This handles cases where semantic search finds documents that keyword search missed
                if let Ok(path_results) = self.keyword_index.search(path, 1) {
                    if let Some(result) = path_results.first() {
                        return result.path == *path && sources.contains(&result.source.as_str());
                    }
                }

                false
            });
        }

        // Fuse results using RRF
        let fused = self.rrf_fusion(&keyword_results, &semantic_results);

        // Return top results
        Ok(fused.into_iter().take(limit).collect())
    }

    /// Perform keyword-only search
    pub fn keyword_search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.keyword_index.search(query, limit)
    }

    /// Perform keyword-only search with source filtering
    #[allow(dead_code)]
    pub fn keyword_search_with_sources(
        &self,
        query: &str,
        limit: usize,
        sources: Option<&[&str]>,
    ) -> Result<Vec<SearchResult>> {
        self.keyword_index.search_with_sources(query, limit, sources)
    }

    /// Perform semantic-only search
    pub fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query_embedding = embed_text(query)?;
        let results = self.vector_index.search(&query_embedding, limit);

        // Convert to SearchResult format
        // Note: We only have path and score from vector search, so we need to look up
        // the full document info from the keyword index
        let mut search_results = Vec::new();
        for (path, score) in results {
            // Try to find matching document in keyword search for full info
            if let Ok(keyword_results) = self.keyword_index.search(&path, 1)
                && let Some(result) = keyword_results.into_iter().next() {
                    search_results.push(SearchResult {
                        score,
                        ..result
                    });
                    continue;
                }
            // Fallback: create minimal result
            search_results.push(SearchResult {
                title: path.clone(),
                snippet: String::new(),
                path,
                source: String::new(),
                score,
            });
        }

        Ok(search_results)
    }

    /// Reciprocal Rank Fusion to combine results from multiple sources
    fn rrf_fusion(
        &self,
        keyword_results: &[SearchResult],
        semantic_results: &[(String, f32)],
    ) -> Vec<SearchResult> {
        // Map from document path to (RRF score, SearchResult)
        let mut scores: HashMap<String, (f32, Option<SearchResult>)> = HashMap::new();

        // Add keyword results with RRF scores
        for (rank, result) in keyword_results.iter().enumerate() {
            let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
            scores
                .entry(result.path.clone())
                .and_modify(|(s, _)| *s += rrf_score)
                .or_insert((rrf_score, Some(result.clone())));
        }

        // Add semantic results with RRF scores
        for (rank, (path, _similarity)) in semantic_results.iter().enumerate() {
            let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
            scores
                .entry(path.clone())
                .and_modify(|(s, _)| *s += rrf_score)
                .or_insert((rrf_score, None));
        }

        // Build final results
        let mut results: Vec<SearchResult> = scores
            .into_iter()
            .map(|(path, (rrf_score, maybe_result))| {
                if let Some(mut result) = maybe_result {
                    result.score = rrf_score;
                    result
                } else {
                    // We have a semantic-only result, try to get full info
                    if let Ok(keyword_results) = self.keyword_index.search(&path, 1)
                        && let Some(mut result) = keyword_results.into_iter().next()
                    {
                        result.score = rrf_score;
                        return result;
                    }
                    // Fallback
                    SearchResult {
                        title: path.clone(),
                        snippet: String::new(),
                        path,
                        source: String::new(),
                        score: rrf_score,
                    }
                }
            })
            .collect();

        // Sort by RRF score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        results
    }
}

/// Search mode for the search tool
#[derive(Debug, Clone, Copy, Default)]
pub enum SearchMode {
    /// Hybrid search (keyword + semantic with RRF fusion)
    #[default]
    Hybrid,
    /// Keyword search only (Tantivy BM25)
    Keyword,
    /// Semantic search only (embedding similarity)
    Semantic,
}

impl SearchMode {
    /// Parse search mode from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "keyword" | "bm25" => SearchMode::Keyword,
            "semantic" | "embedding" | "vector" => SearchMode::Semantic,
            _ => SearchMode::Hybrid,
        }
    }
}
