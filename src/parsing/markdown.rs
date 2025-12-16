use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::path::Path;

use crate::error::Result;

/// A parsed documentation document
#[derive(Debug, Clone)]
pub struct Document {
    /// Document title (first H1 heading or filename)
    pub title: String,
    /// Plain text content (markdown stripped)
    pub content: String,
    /// Relative path to the source file
    pub path: String,
    /// Documentation source (e.g., "rust-book", "rust-reference")
    pub source: String,
}

/// Parse a markdown file and extract its content
pub fn parse_markdown_file(path: &Path, source: &str) -> Result<Document> {
    let markdown = std::fs::read_to_string(path)?;
    let relative_path = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(parse_markdown(&markdown, &relative_path, source))
}

/// Parse markdown content and extract title and plain text
fn parse_markdown(markdown: &str, path: &str, source: &str) -> Document {
    let parser = Parser::new(markdown);

    let mut title: Option<String> = None;
    let mut content = String::new();
    let mut in_heading = false;
    let mut heading_level = 0;
    let mut current_heading = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                in_heading = true;
                heading_level = level as u8;
                current_heading.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                // Use first H1 as title
                if heading_level == 1 && title.is_none() {
                    title = Some(current_heading.clone());
                }
                // Add heading to content
                content.push_str(&current_heading);
                content.push('\n');
            }
            Event::Text(text) | Event::Code(text) => {
                if in_heading {
                    current_heading.push_str(&text);
                } else {
                    content.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                content.push(' ');
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Item) => {
                content.push('\n');
            }
            _ => {}
        }
    }

    Document {
        title: title.unwrap_or_else(|| path.to_string()),
        content: content.trim().to_string(),
        path: path.to_string(),
        source: source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_extracts_title() {
        let md = "# Hello World\n\nThis is content.";
        let doc = parse_markdown(md, "test.md", "test");
        assert_eq!(doc.title, "Hello World");
        assert!(doc.content.contains("This is content"));
    }

    #[test]
    fn test_parse_markdown_strips_formatting() {
        let md = "# Title\n\nSome **bold** and `code` text.";
        let doc = parse_markdown(md, "test.md", "test");
        assert!(doc.content.contains("bold"));
        assert!(doc.content.contains("code"));
        assert!(!doc.content.contains("**"));
        assert!(!doc.content.contains("`"));
    }

    #[test]
    fn test_parse_markdown_fallback_title() {
        let md = "No heading here, just content.";
        let doc = parse_markdown(md, "fallback.md", "test");
        assert_eq!(doc.title, "fallback.md");
    }
}
