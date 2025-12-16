# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

rust-lang-mcp is an MCP (Model Context Protocol) server that provides AI assistants with access to Rust language documentation. It indexes official Rust documentation sources (mdBook/Markdown format) and exposes search functionality via MCP tools.

## Build Commands

```bash
cargo build              # Build debug
cargo build --release    # Build release
cargo run                # Run server (stdio transport)
cargo test               # Run all tests
cargo test test_name     # Run single test
cargo clippy             # Lint
cargo fmt                # Format
```

## Architecture

```
src/
├── main.rs           # Entry point, initializes logging and server
├── server.rs         # MCP server (ServerHandler impl, tool definitions)
├── indexer.rs        # Document collection and indexing orchestration
├── error.rs          # Error types
├── search/
│   ├── mod.rs
│   └── index.rs      # Tantivy search index (indexing + querying)
└── parsing/
    ├── mod.rs
    └── markdown.rs   # Markdown to plain text extraction
```

**Data Flow:**
1. On startup, if index is empty, `indexer.rs` collects markdown files from `data/book/src/`
2. `parsing/markdown.rs` extracts title and content from each file
3. `search/index.rs` indexes documents into Tantivy
4. `server.rs` handles MCP requests and dispatches tool calls to search

## Tech Stack

- **rmcp**: Official MCP SDK for Rust
- **tokio**: Async runtime
- **tantivy**: Full-text search engine
- **pulldown-cmark**: Markdown parsing
- **thiserror/anyhow**: Error handling

## Documentation Sources

Indexed documentation (all mdBook format from GitHub):
- The Rust Book (rust-lang/book) - primary source for Phase 1
- Rust Reference (rust-lang/reference)
- Rust by Example (rust-lang/rust-by-example)
- Rust Design Patterns (rust-unofficial/patterns)
- Rust API Guidelines (rust-lang/api-guidelines)
- Rustonomicon (rust-lang/nomicon)

## Environment Variables

- `RUST_MCP_DATA_DIR`: Data directory for docs and index (default: `./data`)
- `RUST_LOG`: Logging level (e.g., `info`, `debug`)

## Development Phases

1. **Phase 1** (complete): Basic `search_rust_docs` tool, index The Rust Book only
2. **Phase 2**: Add remaining documentation sources
3. **Phase 3**: Semantic search with embeddings
4. **Phase 4**: Specialized tools (`explain_concept`, `get_best_practice`, `show_example`)
