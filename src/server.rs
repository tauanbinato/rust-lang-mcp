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

fn default_limit() -> usize {
    5
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

        let keyword_index = Arc::new(SearchIndex::open_or_create(&index_path)?);
        let vector_index = Arc::new(VectorIndex::open_or_create(&vector_index_path)?);

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

            // Use keyword-only indexing for now (embeddings will be generated on first semantic query)
            let count = indexer::index_all_sources(&keyword_index, &data_dir)?;
            if count > 0 {
                tracing::info!("Keyword indexing complete: {} documents indexed", count);
            } else {
                tracing::warn!("No documentation sources found. Check network connection and try again.");
            }
        }

        // Initialize embedding model if vector index has data (for semantic/hybrid search)
        if !vector_index.is_empty() {
            let models_dir = data_dir.join("models");
            if let Err(e) = init_embedding_model(&models_dir) {
                tracing::warn!("Failed to initialize embedding model: {}. Semantic search will be disabled.", e);
            }
        }

        Ok(Self {
            keyword_index,
            vector_index,
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

    fn tool_box() -> &'static ToolBox<Self> {
        use std::sync::OnceLock;
        static TOOL_BOX: OnceLock<ToolBox<RustDocServer>> = OnceLock::new();
        TOOL_BOX.get_or_init(|| {
            let mut tool_box = ToolBox::new();
            tool_box.add(ToolBoxItem::new(
                Self::search_rust_docs_tool_attr(),
                |context| Box::pin(Self::search_rust_docs_tool_call(context)),
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
