//! In-process server startup shared by the binary and integration tests.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::api::create_router;
use crate::config::ServerConfig;
use crate::db::init_db;
use crate::quic;
use crate::session::SessionManager;

/// A running FinchSpeak server (REST + QUIC) with its bound addresses.
pub struct RunningServer {
    pub http_addr: SocketAddr,
    pub quic_addr: SocketAddr,
    endpoint: quinn::Endpoint,
    http_shutdown: Option<oneshot::Sender<()>>,
    http_task: JoinHandle<()>,
    quic_task: JoinHandle<()>,
}

/// Initialize the database and start the REST and QUIC servers.
///
/// Port `0` in `config.server` binds an ephemeral port; the actual addresses
/// are available on the returned [`RunningServer`].
pub async fn launch(config: ServerConfig) -> Result<RunningServer> {
    tracing::info!("Initializing FinchSpeak database at {}", config.database.url);
    let pool = init_db(&config.database.url)
        .await
        .context("Failed to initialize database")?;

    let sessions = SessionManager::new();

    let http_bind: SocketAddr = format!("{}:{}", config.server.host, config.server.http_port)
        .parse()
        .context("Invalid HTTP bind address")?;
    let listener = tokio::net::TcpListener::bind(http_bind)
        .await
        .with_context(|| format!("Failed to bind HTTP listener on {http_bind}"))?;
    let http_addr = listener.local_addr()?;
    tracing::info!("Axum REST API listening on http://{http_addr}");

    let app = create_router(pool.clone(), config.clone());
    let (http_shutdown, http_shutdown_rx) = oneshot::channel::<()>();
    let http_task = tokio::spawn(async move {
        let shutdown = async {
            let _ = http_shutdown_rx.await;
        };
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
            tracing::error!("HTTP server error: {e}");
        }
    });

    let quic_bind: SocketAddr = format!("{}:{}", config.server.host, config.server.quic_port)
        .parse()
        .context("Invalid QUIC bind address")?;
    let server_config = quic::make_server_config().context("failed to create server config")?;
    let endpoint = quinn::Endpoint::server(server_config, quic_bind)
        .with_context(|| format!("Failed to bind QUIC endpoint on {quic_bind}"))?;
    let quic_addr = endpoint.local_addr()?;
    tracing::info!("QUIC server starting on udp://{quic_addr}");

    let quic_endpoint = endpoint.clone();
    let jwt_secret = config.auth.jwt_secret.clone();
    let quic_task = tokio::spawn(async move {
        if let Err(e) = quic::run_server(quic_endpoint, pool, sessions, jwt_secret).await {
            tracing::error!("QUIC server error: {e}");
        }
    });

    Ok(RunningServer {
        http_addr,
        quic_addr,
        endpoint,
        http_shutdown: Some(http_shutdown),
        http_task,
        quic_task,
    })
}

impl RunningServer {
    /// Resolves when either the REST or the QUIC server task exits on its own.
    pub async fn wait_for_exit(&mut self) {
        tokio::select! {
            res = &mut self.http_task => {
                tracing::error!("HTTP server exited unexpectedly: {res:?}");
            }
            res = &mut self.quic_task => {
                tracing::error!("QUIC server exited unexpectedly: {res:?}");
            }
        }
    }

    /// Stop accepting connections and wait for both server tasks to finish.
    pub async fn shutdown(mut self) {
        self.endpoint.close(0u32.into(), b"shutdown");
        if let Some(tx) = self.http_shutdown.take() {
            let _ = tx.send(());
        }
        let _ = (&mut self.http_task).await;
        let _ = (&mut self.quic_task).await;
        self.endpoint.wait_idle().await;
    }
}
