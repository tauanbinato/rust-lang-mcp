# rust-lang-mcp

An MCP (Model Context Protocol) server that provides AI assistants with access to Rust language documentation through full-text search.

## Setup

### 1. Build the server

```bash
cargo build --release
```

### 2. Download documentation sources

Clone the documentation repositories into the data directory:

```bash
mkdir -p data
cd data

# Required: The Rust Book
git clone --depth 1 https://github.com/rust-lang/book.git

# Optional: Additional sources
git clone --depth 1 https://github.com/rust-lang/reference.git
git clone --depth 1 https://github.com/rust-lang/rust-by-example.git
git clone --depth 1 https://github.com/rust-unofficial/patterns.git
git clone --depth 1 https://github.com/rust-lang/api-guidelines.git
git clone --depth 1 https://github.com/rust-lang/nomicon.git
```

The server indexes any sources present in the data directory.

### 3. Run the server

```bash
cargo run --release
```

The server will automatically index the documentation on first run.

## MCP Configuration

Add to your MCP client configuration:

```json
{
  "mcpServers": {
    "rust-lang-mcp": {
      "command": "/path/to/rust-lang-mcp/target/release/rust-lang-mcp"
    }
  }
}
```

## Tools

### search_rust_docs

Search the indexed Rust documentation.

**Parameters:**
- `query` (string, required): Keywords or phrases to search for
- `limit` (number, optional): Maximum results to return (default: 5, max: 20)

**Example:**
```json
{
  "query": "ownership borrowing",
  "limit": 5
}
```

## Environment Variables

- `RUST_MCP_DATA_DIR`: Override the data directory location (default: `./data`)
- `RUST_LOG`: Set logging level (e.g., `RUST_LOG=info`)
