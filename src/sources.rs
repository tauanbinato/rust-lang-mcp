use std::path::{Path, PathBuf};

use git2::{FetchOptions, RemoteCallbacks};

use crate::error::{Error, Result};

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
        self.repo.split('/').next_back().unwrap_or(self.id)
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
#[allow(dead_code)]
pub fn get_source(id: &str) -> Option<&'static DocSource> {
    DOC_SOURCES.iter().find(|s| s.id == id)
}

/// Clone all documentation sources that don't already exist
pub fn clone_all_sources(data_dir: &Path) -> Result<usize> {
    std::fs::create_dir_all(data_dir)?;

    let mut cloned = 0;

    for source in DOC_SOURCES {
        let target_dir = data_dir.join(source.dir_name());

        if target_dir.exists() {
            tracing::debug!("Source {} already exists at {:?}", source.id, target_dir);
            continue;
        }

        tracing::info!("Cloning {} from {}...", source.name, source.clone_url());

        match clone_repo(&source.clone_url(), &target_dir) {
            Ok(()) => {
                tracing::info!("Successfully cloned {}", source.name);
                cloned += 1;
            }
            Err(e) => {
                tracing::warn!("Failed to clone {}: {}", source.name, e);
            }
        }
    }

    Ok(cloned)
}

/// Clone a single git repository with shallow clone (depth 1)
fn clone_repo(url: &str, target: &Path) -> Result<()> {
    // Set up callbacks for progress reporting
    let mut callbacks = RemoteCallbacks::new();
    callbacks.transfer_progress(|progress| {
        if progress.received_objects() == progress.total_objects() {
            tracing::debug!(
                "Resolving deltas {}/{}",
                progress.indexed_deltas(),
                progress.total_deltas()
            );
        } else {
            tracing::debug!(
                "Received {}/{} objects ({} bytes)",
                progress.received_objects(),
                progress.total_objects(),
                progress.received_bytes()
            );
        }
        true
    });

    // Set up fetch options with depth 1 (shallow clone)
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    fetch_options.depth(1);

    // Build the clone
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_options);

    builder
        .clone(url, target)
        .map_err(|e| Error::Other(format!("Git clone failed: {}", e)))?;

    Ok(())
}
