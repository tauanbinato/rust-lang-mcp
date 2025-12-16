use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    ErrorData as McpError,
};
use rmcp::schemars::{self, JsonSchema};
use rmcp::service::ServiceExt;
use rmcp::transport::io::stdio;
use serde::Deserialize;

use crate::error::Result as CrateResult;
use crate::indexer;
use crate::search::embeddings::init_embedding_model;
use crate::search::{HybridSearch, SearchIndex, SearchMode, VectorIndex};
use crate::sources::clone_all_sources;

/// Parameters for the search_rust_docs tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchDocsParams {
    /// The search query (keywords or phrases to search for)
    pub query: String,
    /// Maximum number of results to return (default: 5, max: 20)
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Search mode: "hybrid" (default, combines keyword + semantic), "keyword" (BM25 only), or "semantic" (embedding similarity only)
    #[serde(default)]
    pub mode: Option<String>,
}

/// Parameters for the explain_concept tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExplainConceptParams {
    /// The Rust concept to explain (e.g., "ownership", "lifetimes", "traits", "borrowing")
    pub concept: String,
    /// Maximum number of documentation sections to return (default: 3)
    #[serde(default = "default_explain_limit")]
    pub limit: usize,
}

/// Parameters for the get_best_practice tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetBestPracticeParams {
    /// The topic to get best practices for (e.g., "error handling", "API design", "naming")
    pub topic: String,
    /// Maximum number of results to return (default: 5)
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Parameters for the show_example tool
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShowExampleParams {
    /// The topic to show examples for (e.g., "iterators", "pattern matching", "closures")
    pub topic: String,
    /// Maximum number of examples to return (default: 3)
    #[serde(default = "default_explain_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    5
}

fn default_explain_limit() -> usize {
    3
}

/// MCP Server for Rust documentation
#[derive(Clone)]
pub struct RustDocServer {
    keyword_index: Arc<SearchIndex>,
    vector_index: Arc<VectorIndex>,
    tool_router: ToolRouter<Self>,
    #[allow(dead_code)]
    data_dir: PathBuf,
}

impl RustDocServer {
    pub async fn new(data_dir: PathBuf) -> CrateResult<Self> {
        let index_path = data_dir.join("index");
        let vector_index_path = index_path.join("vectors");

        let keyword_index = SearchIndex::open_or_create(&index_path)?;
        let mut vector_index = VectorIndex::open_or_create(&vector_index_path)?;

        // Index documents if the keyword index is empty
        if keyword_index.is_empty()? {
            tracing::info!("Index is empty, checking for documentation sources...");

            // Auto-clone documentation sources if they don't exist
            match clone_all_sources(&data_dir) {
                Ok(cloned) if cloned > 0 => {
                    tracing::info!("Cloned {} documentation sources", cloned);
                }
                Ok(_) => {
                    tracing::debug!("All documentation sources already present");
                }
                Err(e) => {
                    tracing::warn!("Failed to clone some sources: {}", e);
                }
            }

            // Index with both keyword and vector indices for hybrid search
            let count = indexer::index_all_sources_hybrid(&keyword_index, &mut vector_index, &data_dir)?;
            if count > 0 {
                tracing::info!("Hybrid indexing complete: {} documents indexed", count);
            } else {
                tracing::warn!("No documentation sources found. Check network connection and try again.");
            }
        }

        // Initialize embedding model for semantic/hybrid search
        if !vector_index.is_empty() {
            let models_dir = data_dir.join("models");
            if let Err(e) = init_embedding_model(&models_dir) {
                tracing::warn!("Failed to initialize embedding model: {}. Semantic search will be disabled.", e);
            }
        }

        Ok(Self {
            keyword_index: Arc::new(keyword_index),
            vector_index: Arc::new(vector_index),
            tool_router: Self::tool_router(),
            data_dir,
        })
    }

    pub async fn run(self) -> anyhow::Result<()> {
        tracing::info!("Starting rust-lang-mcp server on stdio");
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

#[tool_router]
impl RustDocServer {
    #[tool(
        name = "search_rust_docs",
        description = "Search the indexed Rust documentation (The Rust Book, Rust Reference, etc.) for information about Rust concepts, syntax, and best practices. Uses hybrid search (keyword + semantic) by default for best results."
    )]
    async fn search_rust_docs(
        &self,
        Parameters(params): Parameters<SearchDocsParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let limit = if params.limit == 0 {
            5
        } else {
            params.limit.min(20)
        };

        // Determine search mode
        let mode = params
            .mode
            .as_ref()
            .map(|s| SearchMode::from_str(s))
            .unwrap_or_default();

        // Check if we can do semantic/hybrid search
        let can_semantic = !self.vector_index.is_empty();

        let results = if can_semantic {
            let hybrid = HybridSearch::new(&self.keyword_index, &self.vector_index);
            match mode {
                SearchMode::Hybrid => hybrid.search(&params.query, limit),
                SearchMode::Keyword => hybrid.keyword_search(&params.query, limit),
                SearchMode::Semantic => hybrid.semantic_search(&params.query, limit),
            }
        } else {
            // Fall back to keyword-only search
            if !matches!(mode, SearchMode::Keyword) {
                tracing::debug!("Vector index empty, falling back to keyword search");
            }
            self.keyword_index.search(&params.query, limit)
        };

        match results {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "No results found for your query. Try different keywords.",
                    )]));
                }

                let json_results: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|r| {
                        serde_json::json!({
                            "title": r.title,
                            "snippet": r.snippet,
                            "path": r.path,
                            "source": r.source,
                            "score": r.score,
                        })
                    })
                    .collect();

                match serde_json::to_string_pretty(&json_results) {
                    Ok(json) => Ok(CallToolResult::success(vec![Content::text(json)])),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to serialize results: {}",
                        e
                    ))])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {}",
                e
            ))])),
        }
    }

    #[tool(
        name = "explain_concept",
        description = "Get a detailed explanation of a Rust concept. Searches The Rust Book and Rust Reference for comprehensive explanations of concepts like ownership, lifetimes, traits, borrowing, etc."
    )]
    async fn explain_concept(
        &self,
        Parameters(params): Parameters<ExplainConceptParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let limit = if params.limit == 0 { 3 } else { params.limit.min(10) };

        // Search primarily in rust-book and rust-reference
        let sources = ["rust-book", "rust-reference"];
        let hybrid = HybridSearch::new(&self.keyword_index, &self.vector_index);

        let results = if !self.vector_index.is_empty() {
            hybrid.search_with_sources(&params.concept, limit, Some(&sources))
        } else {
            self.keyword_index.search_with_sources(&params.concept, limit, Some(&sources))
        };

        match results {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No documentation found for concept '{}'. Try a different term or check spelling.",
                        params.concept
                    ))]));
                }

                let json_results: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|r| {
                        serde_json::json!({
                            "title": r.title,
                            "explanation": r.snippet,
                            "path": r.path,
                            "source": r.source,
                        })
                    })
                    .collect();

                match serde_json::to_string_pretty(&json_results) {
                    Ok(json) => Ok(CallToolResult::success(vec![Content::text(json)])),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to serialize results: {}", e
                    ))])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {}", e
            ))])),
        }
    }

    #[tool(
        name = "get_best_practice",
        description = "Get Rust best practices and idiomatic patterns for a topic. Searches Rust Design Patterns and API Guidelines for recommendations on error handling, API design, naming conventions, and more."
    )]
    async fn get_best_practice(
        &self,
        Parameters(params): Parameters<GetBestPracticeParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let limit = if params.limit == 0 { 5 } else { params.limit.min(15) };

        // Search in rust-patterns, api-guidelines, and rustonomicon
        let sources = ["rust-patterns", "api-guidelines", "rustonomicon"];
        let hybrid = HybridSearch::new(&self.keyword_index, &self.vector_index);

        let results = if !self.vector_index.is_empty() {
            hybrid.search_with_sources(&params.topic, limit, Some(&sources))
        } else {
            self.keyword_index.search_with_sources(&params.topic, limit, Some(&sources))
        };

        match results {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No best practices found for '{}'. Try searching for related topics like 'error handling', 'API design', or 'naming'.",
                        params.topic
                    ))]));
                }

                let json_results: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|r| {
                        serde_json::json!({
                            "title": r.title,
                            "practice": r.snippet,
                            "path": r.path,
                            "source": r.source,
                        })
                    })
                    .collect();

                match serde_json::to_string_pretty(&json_results) {
                    Ok(json) => Ok(CallToolResult::success(vec![Content::text(json)])),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to serialize results: {}", e
                    ))])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {}", e
            ))])),
        }
    }

    #[tool(
        name = "show_example",
        description = "Get code examples for a Rust topic. Searches Rust by Example for practical, runnable examples demonstrating iterators, pattern matching, closures, error handling, and more."
    )]
    async fn show_example(
        &self,
        Parameters(params): Parameters<ShowExampleParams>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let limit = if params.limit == 0 { 3 } else { params.limit.min(10) };

        // Search primarily in rust-by-example
        let sources = ["rust-by-example"];
        let hybrid = HybridSearch::new(&self.keyword_index, &self.vector_index);

        let results = if !self.vector_index.is_empty() {
            hybrid.search_with_sources(&params.topic, limit, Some(&sources))
        } else {
            self.keyword_index.search_with_sources(&params.topic, limit, Some(&sources))
        };

        match results {
            Ok(results) => {
                if results.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "No examples found for '{}'. Try topics like 'iterators', 'match', 'closures', or 'error handling'.",
                        params.topic
                    ))]));
                }

                let json_results: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|r| {
                        serde_json::json!({
                            "title": r.title,
                            "example": r.snippet,
                            "path": r.path,
                            "source": r.source,
                        })
                    })
                    .collect();

                match serde_json::to_string_pretty(&json_results) {
                    Ok(json) => Ok(CallToolResult::success(vec![Content::text(json)])),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to serialize results: {}", e
                    ))])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {}", e
            ))])),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RustDocServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Rust documentation search server providing access to The Rust Book, Rust Reference, Rust by Example, Design Patterns, API Guidelines, and Rustonomicon.".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
