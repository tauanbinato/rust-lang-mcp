# Parsing Module

This module handles parsing Markdown documentation files and extracting structured content for indexing.

## Overview

The parsing module converts raw Markdown files from various Rust documentation sources into a normalized `Document` structure that can be indexed and searched.

## Components

### `Document` struct

The core data structure representing a parsed document:

```rust
pub struct Document {
    pub title: String,   // Document title (first H1 or filename)
    pub content: String, // Plain text content (markdown stripped)
    pub path: String,    // Relative path to source file
    pub source: String,  // Documentation source (e.g., "rust-book")
}
```

### `parse_markdown_file()`

Main entry point for parsing a file from disk:

```rust
pub fn parse_markdown_file(path: &Path, source: &str) -> Result<Document>
```

- Reads the file contents
- Extracts the filename as the relative path
- Delegates to the internal `parse_markdown()` function

### `parse_markdown()` (internal)

Processes markdown content using `pulldown-cmark`:

1. **Title extraction**: Uses the first H1 heading as the document title, falls back to filename if none found
2. **Content extraction**: Strips all markdown formatting, keeping only plain text
3. **Whitespace normalization**: Converts soft/hard breaks to spaces, adds newlines after paragraphs

## Dependencies

- **pulldown-cmark**: Rust Markdown parser (CommonMark compliant)

## Data Flow

```
┌─────────────────┐
│  Markdown File  │
│  (*.md)         │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ parse_markdown  │
│    _file()      │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ pulldown-cmark  │
│    Parser       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Document     │
│  (title, text)  │
└─────────────────┘
```

## Supported Markdown Elements

| Element | Handling |
|---------|----------|
| Headings | Extracted as text, H1 used for title |
| Paragraphs | Text content preserved |
| Code blocks | Text content preserved (no syntax) |
| Inline code | Text content preserved |
| Bold/Italic | Formatting stripped, text preserved |
| Links | Text preserved, URLs discarded |
| Lists | Text content preserved |
| Soft/Hard breaks | Converted to spaces |

## Usage Example

```rust
use crate::parsing::parse_markdown_file;
use std::path::Path;

let doc = parse_markdown_file(
    Path::new("data/book/src/ch01-00-getting-started.md"),
    "rust-book"
)?;

println!("Title: {}", doc.title);
println!("Content length: {} chars", doc.content.len());
```

## Testing

Run the parsing tests with:

```bash
cargo test parsing
```
