use std::path::{Path, PathBuf};

/// Configuration for a documentation source
#[derive(Debug, Clone)]
pub struct DocSource {
    /// Unique identifier for the source (e.g., "rust-book")
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// GitHub repository (org/repo)
    pub repo: &'static str,
    /// Path to markdown source files within the repo
    pub src_path: &'static str,
}

impl DocSource {
    /// Get the local directory name for this source
    pub fn dir_name(&self) -> &str {
        self.repo.split('/').last().unwrap_or(self.id)
    }

    /// Get the full path to the source files given a data directory
    pub fn docs_path(&self, data_dir: &Path) -> PathBuf {
        data_dir.join(self.dir_name()).join(self.src_path)
    }

    /// Get the git clone URL
    pub fn clone_url(&self) -> String {
        format!("https://github.com/{}.git", self.repo)
    }
}

/// All supported documentation sources
pub const DOC_SOURCES: &[DocSource] = &[
    DocSource {
        id: "rust-book",
        name: "The Rust Programming Language",
        repo: "rust-lang/book",
        src_path: "src",
    },
    DocSource {
        id: "rust-reference",
        name: "The Rust Reference",
        repo: "rust-lang/reference",
        src_path: "src",
    },
    DocSource {
        id: "rust-by-example",
        name: "Rust by Example",
        repo: "rust-lang/rust-by-example",
        src_path: "src",
    },
    DocSource {
        id: "rust-patterns",
        name: "Rust Design Patterns",
        repo: "rust-unofficial/patterns",
        src_path: "src",
    },
    DocSource {
        id: "api-guidelines",
        name: "Rust API Guidelines",
        repo: "rust-lang/api-guidelines",
        src_path: "src",
    },
    DocSource {
        id: "rustonomicon",
        name: "The Rustonomicon",
        repo: "rust-lang/nomicon",
        src_path: "src",
    },
];

/// Get a documentation source by ID
pub fn get_source(id: &str) -> Option<&'static DocSource> {
    DOC_SOURCES.iter().find(|s| s.id == id)
}
