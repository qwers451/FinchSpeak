//! # FinchSpeak Server
//!
//! High-performance QUIC SFU and Axum REST API server for FinchSpeak.

use std::path::Path;

use anyhow::{Context, Result};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use finch_server::config::ServerConfig;
use finch_server::launch::launch;

/// Log to the console and to daily files in `config.logging.dir` (old files are deleted).
/// The returned guard flushes the file on exit.
fn init_logging(config: &ServerConfig) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,finch_server=debug"));
    let console = tracing_subscriber::fmt::layer().with_filter(filter());

    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("finch-server")
        .filename_suffix("log")
        .max_log_files(config.logging.keep_files.max(1))
        .build(&config.logging.dir);
    match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer).with_filter(filter());
            tracing_subscriber::registry().with(console).with(file).init();
            Some(guard)
        }
        Err(e) => {
            tracing_subscriber::registry().with(console).init();
            tracing::warn!("Log files are disabled ({e}); logging to the console only");
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load configuration
    let config_path = Path::new("finch-server.toml");
    let config = ServerConfig::load_or_default(Some(config_path))
        .context("Failed to load server configuration")?;
    let _log_guard = init_logging(&config);

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
