# rust-lang-mcp

An MCP (Model Context Protocol) server that provides AI assistants with access to Rust language documentation through hybrid search (keyword + semantic).

## Features

- **Full-text search** using Tantivy (BM25 ranking)
- **Semantic search** using local ONNX embeddings (all-MiniLM-L6-v2)
- **Hybrid search** combining both methods with Reciprocal Rank Fusion (RRF)
- **Multiple documentation sources**: The Rust Book, Rust Reference, Rust by Example, Design Patterns, API Guidelines, and Rustonomicon

## Setup

### 1. Build the server

```bash
cargo build --release
```

### 2. Run the server

```bash
./target/release/rust-lang-mcp
```

On first run, the server will automatically:
1. Clone all documentation repositories (shallow clone, ~50MB total)
2. Index all markdown files for search

This takes about 1-2 minutes on first startup. Subsequent runs are instant.

### Manual documentation setup (optional)

If you prefer to clone the repositories manually or the auto-clone fails:

```bash
mkdir -p data
cd data

git clone --depth 1 https://github.com/rust-lang/book.git
git clone --depth 1 https://github.com/rust-lang/reference.git
git clone --depth 1 https://github.com/rust-lang/rust-by-example.git
git clone --depth 1 https://github.com/rust-unofficial/patterns.git
git clone --depth 1 https://github.com/rust-lang/api-guidelines.git
git clone --depth 1 https://github.com/rust-lang/nomicon.git
```

## MCP Client Configuration

### Claude Desktop

Add to `~/.config/claude/claude_desktop_config.json` (Linux) or `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS):

```json
{
  "mcpServers": {
    "rust-docs": {
      "command": "/absolute/path/to/rust-lang-mcp/target/release/rust-lang-mcp"
    }
  }
}
```

### Claude Code (CLI)

#### Global configuration (recommended)

Add to `~/.claude.json` to make the MCP available in all projects:

```json
{
  "mcpServers": {
    "rust-docs": {
      "command": "/absolute/path/to/rust-lang-mcp/target/release/rust-lang-mcp"
    }
  }
}
```

> **Note:** Add the `mcpServers` key at the top level of the existing `~/.claude.json` file (don't replace the whole file).

#### Project-level configuration

For project-specific setup, create a `.mcp.json` file in the project root:

```json
{
  "mcpServers": {
    "rust-lang-mcp": {
      "command": "cargo",
      "args": ["run", "--release"],
      "cwd": "/absolute/path/to/rust-lang-mcp"
    }
  }
}
```

This runs the MCP server directly from source, useful during development.

> **Note:** You can check your MCP config locations by running `/mcp` in Claude Code. User-level config is at `~/.claude.json`, project-level at `.mcp.json`.

### Cursor

Add to Cursor's MCP settings:

```json
{
  "mcpServers": {
    "rust-docs": {
      "command": "/absolute/path/to/rust-lang-mcp/target/release/rust-lang-mcp"
    }
  }
}
```

## Tools

### search_rust_docs

Search the indexed Rust documentation for concepts, syntax, and best practices.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | Yes | - | Keywords or phrases to search for |
| `limit` | number | No | 5 | Maximum results to return (max: 20) |
| `mode` | string | No | "hybrid" | Search mode: `"hybrid"`, `"keyword"`, or `"semantic"` |

**Search Modes:**

- `hybrid` (default): Combines keyword and semantic search using RRF score fusion. Best for most queries.
- `keyword`: Traditional BM25 keyword search. Best for exact term matching.
- `semantic`: Embedding-based similarity search. Best for conceptual queries.

**Example:**

```json
{
  "query": "how to handle errors with Result",
  "limit": 5,
  "mode": "hybrid"
}
```

**Response:**

```json
[
  {
    "title": "Recoverable Errors with Result",
    "snippet": "Most errors aren't serious enough to require the program to stop entirely...",
    "path": "ch09-02-recoverable-errors-with-result.md",
    "source": "rust-book",
    "score": 0.032
  }
]
```

## Documentation Sources

| Source | Repository | Description |
|--------|------------|-------------|
| The Rust Book | rust-lang/book | Official Rust programming language book |
| Rust Reference | rust-lang/reference | Detailed language reference |
| Rust by Example | rust-lang/rust-by-example | Learn Rust through examples |
| Design Patterns | rust-unofficial/patterns | Common Rust design patterns and idioms |
| API Guidelines | rust-lang/api-guidelines | Rust API design recommendations |
| Rustonomicon | rust-lang/nomicon | The Dark Arts of Unsafe Rust |

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_MCP_DATA_DIR` | `./data` | Directory containing documentation and index |
| `RUST_LOG` | - | Logging level (e.g., `info`, `debug`, `trace`) |

## How It Works

1. **Indexing**: On first run, the server parses all Markdown files from the documentation sources and builds a Tantivy full-text index.

2. **Keyword Search**: Uses Tantivy's BM25 algorithm to find documents matching query terms.

3. **Semantic Search** (when enabled):
   - Generates 384-dimensional embeddings using the all-MiniLM-L6-v2 ONNX model
   - Stores embeddings in an HNSW (Hierarchical Navigable Small World) index
   - Finds semantically similar documents even without exact keyword matches

4. **Hybrid Search**: Runs both searches in parallel and merges results using Reciprocal Rank Fusion (RRF), which combines rankings from multiple sources effectively.

## Development

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=info cargo run --release

# Build debug version
cargo build
```

## License

MIT
