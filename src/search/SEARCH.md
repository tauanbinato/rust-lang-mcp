# Search Module

This module provides the search functionality for the MCP server, supporting keyword, semantic, and hybrid search modes.

## Overview

The search module implements a sophisticated search system that combines:
- **Keyword search**: Traditional BM25 full-text search using Tantivy
- **Semantic search**: Embedding-based similarity search using ONNX + HNSW
- **Hybrid search**: Combines both using Reciprocal Rank Fusion (RRF)

## Components

### 1. Keyword Search (`index.rs`)

Full-text search powered by Tantivy (Rust's Lucene equivalent).

#### `SearchIndex`

```rust
pub struct SearchIndex {
    index: Index,
    schema: Schema,
}
```

**Schema fields:**
- `title` - Document title (TEXT + STORED)
- `content` - Full document content (TEXT + STORED)
- `path` - File path (STORED)
- `source` - Documentation source (STORED)

**Key methods:**
- `open_or_create(path)` - Open existing or create new index
- `index_documents(docs)` - Index a batch of documents
- `search(query, limit)` - Execute BM25 search
- `is_empty()` - Check if index needs populating

### 2. Semantic Search

#### Embeddings (`embeddings.rs`)

Uses the `all-MiniLM-L6-v2` model for generating 384-dimensional embeddings.

**Features:**
- ONNX Runtime for fast inference
- Automatic model download from Hugging Face
- Mean pooling + L2 normalization
- Batch processing support

```rust
// Generate embedding for a query
let embedding = embed_text("ownership and borrowing")?;
```

#### Vector Index (`vector_index.rs`)

HNSW (Hierarchical Navigable Small World) index for fast approximate nearest neighbor search.

**Features:**
- Cosine similarity metric
- Parallel batch insertion
- JSON persistence (rebuilds HNSW on load)

```rust
let mut index = VectorIndex::new();
index.add("doc.md".to_string(), embedding);
let results = index.search(&query_embedding, 10);
```

### 3. Hybrid Search (`hybrid.rs`)

Combines keyword and semantic search using Reciprocal Rank Fusion.

#### `HybridSearch`

```rust
pub struct HybridSearch<'a> {
    keyword_index: &'a SearchIndex,
    vector_index: &'a VectorIndex,
}
```

**RRF Score Formula:**
```
RRF(d) = Σ 1 / (k + rank(d))
```
Where `k = 60` (standard constant from the original RRF paper).

#### Search Modes

```rust
pub enum SearchMode {
    Hybrid,   // Default: keyword + semantic with RRF
    Keyword,  // BM25 only
    Semantic, // Embedding similarity only
}
```

## Architecture

```
                    ┌─────────────────┐
                    │   User Query    │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │  HybridSearch   │
                    └────────┬────────┘
                             │
              ┌──────────────┴──────────────┐
              │                             │
              ▼                             ▼
     ┌─────────────────┐           ┌─────────────────┐
     │  SearchIndex    │           │  VectorIndex    │
     │  (Tantivy BM25) │           │  (HNSW)         │
     └────────┬────────┘           └────────┬────────┘
              │                             │
              │                             ▼
              │                    ┌─────────────────┐
              │                    │ EmbeddingModel  │
              │                    │ (ONNX Runtime)  │
              │                    └────────┬────────┘
              │                             │
              └──────────────┬──────────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │   RRF Fusion    │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │ SearchResult[]  │
                    └─────────────────┘
```

## SearchResult

```rust
pub struct SearchResult {
    pub title: String,   // Document title
    pub snippet: String, // Relevant text excerpt
    pub path: String,    // File path
    pub source: String,  // Documentation source
    pub score: f32,      // Relevance score
}
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| tantivy | Full-text search engine |
| ort | ONNX Runtime bindings |
| tokenizers | HuggingFace tokenizer |
| hnsw_rs | HNSW vector index |
| ndarray | N-dimensional arrays for embeddings |

## Storage Layout

```
data/
├── index/           # Tantivy keyword index
│   └── ...
├── vector_index/    # HNSW vector index
│   └── vector_index.json
└── models/          # Embedding model files
    ├── model.onnx
    └── tokenizer.json
```

## Usage Example

```rust
use crate::search::{HybridSearch, SearchIndex, VectorIndex, SearchMode};

// Load indexes
let keyword_index = SearchIndex::open_or_create(&index_path)?;
let vector_index = VectorIndex::open_or_create(&vector_path)?;

// Create hybrid search
let search = HybridSearch::new(&keyword_index, &vector_index);

// Search
let results = search.search("lifetime annotations", 5)?;
```

## Testing

```bash
# Run all search tests
cargo test search

# Run with model download (slow)
cargo test --ignored
```
