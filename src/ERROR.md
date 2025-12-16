# Error Handling

This document describes the error handling approach used in rust-lang-mcp.

## Overview

The project uses a centralized error type with the `thiserror` crate for ergonomic error definitions and automatic `From` implementations.

## Error Type

All errors are represented by the `Error` enum in `error.rs`:

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),

    #[error("Query parse error: {0}")]
    QueryParse(#[from] tantivy::query::QueryParserError),

    #[error("ONNX Runtime error: {0}")]
    Ort(#[from] ort::Error),

    #[error("Index not found at {0}")]
    IndexNotFound(String),

    #[error("Documentation directory not found at {0}")]
    DocsNotFound(String),

    #[error("{0}")]
    Other(String),
}
```

## Result Type Alias

A convenient type alias is provided:

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## Error Variants

| Variant | Source | When |
|---------|--------|------|
| `Io` | `std::io::Error` | File operations, directory creation |
| `Tantivy` | `tantivy::TantivyError` | Index operations, writer errors |
| `QueryParse` | `tantivy::QueryParserError` | Invalid search queries |
| `Ort` | `ort::Error` | ONNX model loading/inference |
| `IndexNotFound` | Manual | Missing index directory |
| `DocsNotFound` | Manual | Missing documentation directory |
| `Other` | Manual | Catch-all for misc errors |

## Usage Patterns

### Using the `?` Operator

Thanks to `#[from]` attributes, errors automatically convert:

```rust
use crate::error::Result;

fn read_docs(path: &Path) -> Result<String> {
    // std::io::Error automatically converts to Error::Io
    let content = std::fs::read_to_string(path)?;
    Ok(content)
}
```

### Creating Custom Errors

```rust
use crate::error::Error;

fn validate_something(x: i32) -> Result<()> {
    if x < 0 {
        return Err(Error::Other("Value must be positive".to_string()));
    }
    Ok(())
}
```

### Domain-Specific Errors

```rust
fn find_docs(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(Error::DocsNotFound(path.display().to_string()));
    }
    Ok(())
}
```

## Error Propagation

Errors flow up through the call stack:

```
┌─────────────────┐
│   MCP Handler   │  ← Returns JSON error to client
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Search Layer   │  ← Propagates with ?
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Index Layer    │  ← Propagates with ?
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Tantivy      │  ← Original error source
└─────────────────┘
```

## Dependencies

- **thiserror**: Derive macro for `std::error::Error`

## Best Practices

1. **Prefer specific variants** over `Error::Other` when possible
2. **Use `?` operator** for automatic error propagation
3. **Add context** when converting with `map_err`:
   ```rust
   serde_json::to_string(&data)
       .map_err(|e| Error::Other(format!("JSON serialization failed: {}", e)))?;
   ```
4. **Log errors** at appropriate levels before returning

## Testing

Errors can be tested using pattern matching:

```rust
#[test]
fn test_missing_docs() {
    let result = find_docs(Path::new("/nonexistent"));
    assert!(matches!(result, Err(Error::DocsNotFound(_))));
}
```
