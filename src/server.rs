use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::tool::{ToolBox, ToolBoxItem, ToolCallContext};
use rmcp::model::{
    CallToolRequestParam, CallToolResult, Implementation, ListToolsResult, PaginatedRequestParam,
    ServerCapabilities, ServerInfo, ToolsCapability,
};
use rmcp::schemars::{self, JsonSchema};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{serve_server, transport::io::stdio, Error as McpError, ServerHandler};
use serde::Deserialize;

use crate::error::Result;
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
    #[allow(dead_code)]
    data_dir: PathBuf,
}

impl RustDocServer {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
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
            data_dir,
        })
    }

    pub async fn run(self) -> Result<()> {
        tracing::info!("Starting rust-lang-mcp server on stdio");
        let transport = stdio();
        let _service = serve_server(self, transport).await?;
        // Keep running until terminated
        tokio::signal::ctrl_c().await.ok();
        Ok(())
    }

    // Tool definition for search_rust_docs
    fn search_rust_docs_tool_attr() -> rmcp::model::Tool {
        use rmcp::handler::server::tool::cached_schema_for_type;
        rmcp::model::Tool {
            name: "search_rust_docs".into(),
            description: "Search the indexed Rust documentation (The Rust Book, Rust Reference, etc.) for information about Rust concepts, syntax, and best practices. Uses hybrid search (keyword + semantic) by default for best results.".into(),
            input_schema: cached_schema_for_type::<SearchDocsParams>(),
        }
    }

    async fn search_rust_docs_tool_call(
        context: ToolCallContext<'_, Self>,
    ) -> std::result::Result<CallToolResult, McpError> {
        use rmcp::handler::server::tool::FromToolCallContextPart;
        use rmcp::handler::server::tool::Parameters;
        use rmcp::model::Content;

        let (callee, context) =
            <&Self as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;
        let (Parameters(params), _context) =
            <Parameters<SearchDocsParams> as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;

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
        let can_semantic = !callee.vector_index.is_empty();

        let results = if can_semantic {
            let hybrid = HybridSearch::new(&callee.keyword_index, &callee.vector_index);
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
            callee.keyword_index.search(&params.query, limit)
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

    // Tool definition for explain_concept
    fn explain_concept_tool_attr() -> rmcp::model::Tool {
        use rmcp::handler::server::tool::cached_schema_for_type;
        rmcp::model::Tool {
            name: "explain_concept".into(),
            description: "Get a detailed explanation of a Rust concept. Searches The Rust Book and Rust Reference for comprehensive explanations of concepts like ownership, lifetimes, traits, borrowing, etc.".into(),
            input_schema: cached_schema_for_type::<ExplainConceptParams>(),
        }
    }

    async fn explain_concept_tool_call(
        context: ToolCallContext<'_, Self>,
    ) -> std::result::Result<CallToolResult, McpError> {
        use rmcp::handler::server::tool::FromToolCallContextPart;
        use rmcp::handler::server::tool::Parameters;
        use rmcp::model::Content;

        let (callee, context) =
            <&Self as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;
        let (Parameters(params), _context) =
            <Parameters<ExplainConceptParams> as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;

        let limit = if params.limit == 0 { 3 } else { params.limit.min(10) };

        // Search primarily in rust-book and rust-reference
        let sources = ["rust-book", "rust-reference"];
        let hybrid = HybridSearch::new(&callee.keyword_index, &callee.vector_index);

        let results = if !callee.vector_index.is_empty() {
            hybrid.search_with_sources(&params.concept, limit, Some(&sources))
        } else {
            callee.keyword_index.search_with_sources(&params.concept, limit, Some(&sources))
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

    // Tool definition for get_best_practice
    fn get_best_practice_tool_attr() -> rmcp::model::Tool {
        use rmcp::handler::server::tool::cached_schema_for_type;
        rmcp::model::Tool {
            name: "get_best_practice".into(),
            description: "Get Rust best practices and idiomatic patterns for a topic. Searches Rust Design Patterns and API Guidelines for recommendations on error handling, API design, naming conventions, and more.".into(),
            input_schema: cached_schema_for_type::<GetBestPracticeParams>(),
        }
    }

    async fn get_best_practice_tool_call(
        context: ToolCallContext<'_, Self>,
    ) -> std::result::Result<CallToolResult, McpError> {
        use rmcp::handler::server::tool::FromToolCallContextPart;
        use rmcp::handler::server::tool::Parameters;
        use rmcp::model::Content;

        let (callee, context) =
            <&Self as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;
        let (Parameters(params), _context) =
            <Parameters<GetBestPracticeParams> as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;

        let limit = if params.limit == 0 { 5 } else { params.limit.min(15) };

        // Search in rust-patterns, api-guidelines, and rustonomicon
        let sources = ["rust-patterns", "api-guidelines", "rustonomicon"];
        let hybrid = HybridSearch::new(&callee.keyword_index, &callee.vector_index);

        let results = if !callee.vector_index.is_empty() {
            hybrid.search_with_sources(&params.topic, limit, Some(&sources))
        } else {
            callee.keyword_index.search_with_sources(&params.topic, limit, Some(&sources))
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

    // Tool definition for show_example
    fn show_example_tool_attr() -> rmcp::model::Tool {
        use rmcp::handler::server::tool::cached_schema_for_type;
        rmcp::model::Tool {
            name: "show_example".into(),
            description: "Get code examples for a Rust topic. Searches Rust by Example for practical, runnable examples demonstrating iterators, pattern matching, closures, error handling, and more.".into(),
            input_schema: cached_schema_for_type::<ShowExampleParams>(),
        }
    }

    async fn show_example_tool_call(
        context: ToolCallContext<'_, Self>,
    ) -> std::result::Result<CallToolResult, McpError> {
        use rmcp::handler::server::tool::FromToolCallContextPart;
        use rmcp::handler::server::tool::Parameters;
        use rmcp::model::Content;

        let (callee, context) =
            <&Self as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;
        let (Parameters(params), _context) =
            <Parameters<ShowExampleParams> as FromToolCallContextPart<'_, Self>>::from_tool_call_context_part(context)?;

        let limit = if params.limit == 0 { 3 } else { params.limit.min(10) };

        // Search primarily in rust-by-example
        let sources = ["rust-by-example"];
        let hybrid = HybridSearch::new(&callee.keyword_index, &callee.vector_index);

        let results = if !callee.vector_index.is_empty() {
            hybrid.search_with_sources(&params.topic, limit, Some(&sources))
        } else {
            callee.keyword_index.search_with_sources(&params.topic, limit, Some(&sources))
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

    fn tool_box() -> &'static ToolBox<Self> {
        use std::sync::OnceLock;
        static TOOL_BOX: OnceLock<ToolBox<RustDocServer>> = OnceLock::new();
        TOOL_BOX.get_or_init(|| {
            let mut tool_box = ToolBox::new();
            tool_box.add(ToolBoxItem::new(
                Self::search_rust_docs_tool_attr(),
                |context| Box::pin(Self::search_rust_docs_tool_call(context)),
            ));
            tool_box.add(ToolBoxItem::new(
                Self::explain_concept_tool_attr(),
                |context| Box::pin(Self::explain_concept_tool_call(context)),
            ));
            tool_box.add(ToolBoxItem::new(
                Self::get_best_practice_tool_attr(),
                |context| Box::pin(Self::get_best_practice_tool_call(context)),
            ));
            tool_box.add(ToolBoxItem::new(
                Self::show_example_tool_attr(),
                |context| Box::pin(Self::show_example_tool_call(context)),
            ));
            tool_box
        })
    }
}

impl ServerHandler for RustDocServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: None,
                }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "rust-lang-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: None,
        }
    }

    async fn list_tools(
        &self,
        _request: PaginatedRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            next_cursor: None,
            tools: Self::tool_box().list(),
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let tool_context = ToolCallContext::new(self, request, context);
        Self::tool_box().call(tool_context).await
    }
}
