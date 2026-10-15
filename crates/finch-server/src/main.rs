//! # FinchSpeak Server
//!
//! High-performance QUIC SFU and Axum REST API server for FinchSpeak.

use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use finch_server::api::create_router;
use finch_server::config::ServerConfig;
use finch_server::db::init_db;
use finch_server::quic;
use finch_server::session::SessionManager;

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

    tracing::info!("Initializing FinchSpeak database at {}", config.database.url);

    // 2. Initialize SQLite pool and run migrations
    let pool = init_db(&config.database.url)
        .await
        .context("Failed to initialize database")?;

    // 3. Initialize SessionManager
    let sessions = SessionManager::new();

    // 4. Start Axum HTTP REST API Server
    let http_addr: SocketAddr = format!("{}:{}", config.server.host, config.server.http_port)
        .parse()
        .context("Invalid HTTP bind address")?;

    let app = create_router(pool.clone(), config.clone());
    let listener = tokio::net::TcpListener::bind(http_addr)
        .await
        .with_context(|| format!("Failed to bind HTTP listener on {http_addr}"))?;

    tracing::info!("Axum REST API listening on http://{http_addr}");

    let http_server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("HTTP server error: {e}");
        }
    });

    // 5. Start QUIC Media & Signaling Server
    let quic_addr: SocketAddr = format!("{}:{}", config.server.host, config.server.quic_port)
        .parse()
        .context("Invalid QUIC bind address")?;

    tracing::info!("QUIC server starting on udp://{quic_addr}");
    let pool_quic = pool.clone();
    let sessions_quic = sessions.clone();
    let jwt_secret = config.auth.jwt_secret.clone();

    let quic_server = tokio::spawn(async move {
        if let Err(e) = quic::run_server(quic_addr, pool_quic, sessions_quic, jwt_secret).await {
            tracing::error!("QUIC server error: {e}");
        }
    });

    // 6. Wait for shutdown signal (Ctrl+C)
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown signal received, terminating server...");
        }
        res = http_server => {
            tracing::error!("HTTP server exited unexpectedly: {res:?}");
        }
        res = quic_server => {
            tracing::error!("QUIC server exited unexpectedly: {res:?}");
        }
    }

    Ok(())
}
