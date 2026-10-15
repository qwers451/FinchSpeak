//! # FinchSpeak Server
//!
//! High-performance QUIC SFU and Axum REST API server for FinchSpeak.

use std::path::Path;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use finch_server::config::ServerConfig;
use finch_server::launch::launch;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,finch_server=debug")),
        )
        .init();

    // 1. Load configuration
    let config_path = Path::new("finch-server.toml");
    let config = ServerConfig::load_or_default(Some(config_path))
        .context("Failed to load server configuration")?;

    // 2. Initialize database, REST API and QUIC server
    let mut server = launch(config).await?;

    // 3. Wait for shutdown signal (Ctrl+C) or an unexpected server exit
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown signal received, terminating server...");
        }
        _ = server.wait_for_exit() => {}
    }

    server.shutdown().await;
    Ok(())
}
