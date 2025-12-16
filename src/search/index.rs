use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, STORED, TEXT};
use tantivy::{doc, Index, IndexWriter, TantivyDocument};

use crate::error::Result;
use crate::parsing::Document;

/// Search result returned to users
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub snippet: String,
    pub path: String,
    pub source: String,
    pub score: f32,
}

/// Tantivy-based search index for documentation
pub struct SearchIndex {
    index: Index,
    schema: Schema,
}

impl SearchIndex {
    /// Create or open an index at the given path
    pub fn open_or_create(index_path: &Path) -> Result<Self> {
        let schema = Self::build_schema();

        let index = if index_path.exists() {
            Index::open_in_dir(index_path)?
        } else {
            std::fs::create_dir_all(index_path)?;
            Index::create_in_dir(index_path, schema.clone())?
        };

        Ok(Self { index, schema })
    }

    /// Create an in-memory index (for testing)
    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let schema = Self::build_schema();
        let index = Index::create_in_ram(schema.clone());
        Ok(Self { index, schema })
    }

    fn build_schema() -> Schema {
        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("title", TEXT | STORED);
        schema_builder.add_text_field("content", TEXT | STORED);
        schema_builder.add_text_field("path", STORED);
        schema_builder.add_text_field("source", STORED);
        schema_builder.build()
    }

    /// Index a batch of documents
    pub fn index_documents(&self, documents: &[Document]) -> Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;

        let title_field = self.schema.get_field("title").unwrap();
        let content_field = self.schema.get_field("content").unwrap();
        let path_field = self.schema.get_field("path").unwrap();
        let source_field = self.schema.get_field("source").unwrap();

        // Clear existing documents
        writer.delete_all_documents()?;

        for doc in documents {
            writer.add_document(doc!(
                title_field => doc.title.clone(),
                content_field => doc.content.clone(),
                path_field => doc.path.clone(),
                source_field => doc.source.clone(),
            ))?;
        }

        writer.commit()?;
        Ok(())
    }

    /// Search the index and return top results
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let title_field = self.schema.get_field("title").unwrap();
        let content_field = self.schema.get_field("content").unwrap();
        let path_field = self.schema.get_field("path").unwrap();
        let source_field = self.schema.get_field("source").unwrap();

        let query_parser = QueryParser::for_index(&self.index, vec![title_field, content_field]);
        let query = query_parser.parse_query(query_str)?;

        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;

            let title = doc
                .get_first(title_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let content = doc
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let path = doc
                .get_first(path_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let source = doc
                .get_first(source_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Extract a snippet around the query terms
            let snippet = Self::extract_snippet(content, query_str, 200);

            results.push(SearchResult {
                title,
                snippet,
                path,
                source,
                score,
            });
        }

        Ok(results)
    }

    /// Check if the index has any documents
    pub fn is_empty(&self) -> Result<bool> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        Ok(searcher.num_docs() == 0)
    }

    /// Extract a snippet of text around query terms
    fn extract_snippet(content: &str, query: &str, max_len: usize) -> String {
        let query_lower = query.to_lowercase();
        let content_lower = content.to_lowercase();

        // Find the first occurrence of any query word
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let mut best_pos = 0;

        for word in &query_words {
            if let Some(pos) = content_lower.find(word) {
                best_pos = pos;
                break;
            }
        }

        // Extract snippet around the found position
        let start = best_pos.saturating_sub(max_len / 2);
        let end = (start + max_len).min(content.len());

        let mut snippet: String = content
            .chars()
            .skip(start)
            .take(end - start)
            .collect();

        // Clean up snippet boundaries
        if start > 0
            && let Some(space_pos) = snippet.find(' ') {
                snippet = snippet[space_pos + 1..].to_string();
                snippet.insert_str(0, "...");
            }
        if end < content.len()
            && let Some(space_pos) = snippet.rfind(' ') {
                snippet.truncate(space_pos);
                snippet.push_str("...");
            }

        snippet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_and_search() -> Result<()> {
        let index = SearchIndex::in_memory()?;

        let docs = vec![
            Document {
                title: "Ownership".to_string(),
                content: "Rust uses ownership to manage memory safely.".to_string(),
                path: "ownership.md".to_string(),
                source: "rust-book".to_string(),
            },
            Document {
                title: "Borrowing".to_string(),
                content: "Borrowing allows references without taking ownership.".to_string(),
                path: "borrowing.md".to_string(),
                source: "rust-book".to_string(),
            },
        ];

        index.index_documents(&docs)?;

        let results = index.search("ownership", 10)?;
        assert!(!results.is_empty());
        assert!(results[0].title.contains("Ownership") || results[0].snippet.contains("ownership"));

        Ok(())
    }

    #[test]
    fn test_empty_index() -> Result<()> {
        let index = SearchIndex::in_memory()?;
        assert!(index.is_empty()?);
        Ok(())
    }
}
