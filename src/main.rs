mod error;
mod indexer;
mod parsing;
mod search;
mod server;
mod sources;

use std::path::PathBuf;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is used for MCP communication)
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Data directory for docs and index
    let data_dir = std::env::var("RUST_MCP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"));

    let server = server::RustDocServer::new(data_dir).await?;
    server.run().await?;

    Ok(())
}
