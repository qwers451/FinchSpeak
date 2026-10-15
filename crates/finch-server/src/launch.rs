//! In-process server startup shared by the binary and integration tests.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::api::create_router;
use crate::config::ServerConfig;
use crate::db::init_db;
use crate::https;
use crate::quic;
use crate::session::SessionManager;

/// A running FinchSpeak server (REST + QUIC) with its bound addresses.
pub struct RunningServer {
    /// Address of the HTTPS REST listener.
    pub http_addr: SocketAddr,
    pub quic_addr: SocketAddr,
    /// SHA-256 fingerprint of the certificate used by both REST and QUIC (what clients pin).
    pub certificate_fingerprint: String,
    endpoint: quinn::Endpoint,
    http_shutdown: Option<oneshot::Sender<()>>,
    http_task: Option<JoinHandle<()>>,
    quic_task: Option<JoinHandle<()>>,
}

/// Initialize the database and start the REST and QUIC servers.
///
/// Port `0` in `config.server` binds an ephemeral port; the actual addresses
/// are available on the returned [`RunningServer`].
pub async fn launch(mut config: ServerConfig) -> Result<RunningServer> {
    config.auth.jwt_secret = config
        .auth
        .resolve_jwt_secret()
        .context("Failed to resolve JWT secret")?;

    tracing::info!("Initializing FinchSpeak database at {}", config.database.url);
    let pool = init_db(&config.database.url)
        .await
        .context("Failed to initialize database")?;

    let sessions = SessionManager::new();

    let quic_bind: SocketAddr = format!("{}:{}", config.server.host, config.server.quic_port)
        .parse()
        .context("Invalid QUIC bind address")?;
    let (cert, key) = quic::load_or_create_cert(std::path::Path::new(&config.server.tls_dir))
        .context("Failed to load TLS certificate")?;
    let certificate_fingerprint = quic::certificate_fingerprint(&cert);
    let https_config = https::make_tls_config(cert.clone(), key.clone_key())?;
    let server_config = quic::make_server_config_with(cert, key).context("failed to create server config")?;
    let endpoint = quinn::Endpoint::server(server_config, quic_bind)
        .with_context(|| format!("Failed to bind QUIC endpoint on {quic_bind}"))?;
    let quic_addr = endpoint.local_addr()?;
    tracing::info!("QUIC server starting on udp://{quic_addr}");

    // Clients learn the QUIC port from the login response.
    config.server.quic_port = quic_addr.port();

    let http_bind: SocketAddr = format!("{}:{}", config.server.host, config.server.http_port)
        .parse()
        .context("Invalid HTTP bind address")?;
    let listener = tokio::net::TcpListener::bind(http_bind)
        .await
        .with_context(|| format!("Failed to bind HTTP listener on {http_bind}"))?;
    let http_addr = listener.local_addr()?;
    tracing::info!("REST API listening on https://{http_addr} (certificate {certificate_fingerprint})");

    let app = create_router(pool.clone(), config.clone(), sessions.clone());
    let (http_shutdown, http_shutdown_rx) = oneshot::channel::<()>();
    let http_task = tokio::spawn(https::serve(listener, https_config, app, http_shutdown_rx));

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
        certificate_fingerprint,
        endpoint,
        http_shutdown: Some(http_shutdown),
        http_task: Some(http_task),
        quic_task: Some(quic_task),
    })
}

impl RunningServer {
    /// Resolves when either the REST or the QUIC server task exits on its own.
    pub async fn wait_for_exit(&mut self) {
        // If one server is already gone there is nothing left to wait for.
        if let (Some(http), Some(quic)) = (&mut self.http_task, &mut self.quic_task) {
            tokio::select! {
                res = http => {
                    tracing::error!("HTTP server exited unexpectedly: {res:?}");
                    self.http_task = None;
                }
                res = quic => {
                    tracing::error!("QUIC server exited unexpectedly: {res:?}");
                    self.quic_task = None;
                }
            }
        }
    }

    /// Stop accepting connections and wait for both server tasks to finish.
    pub async fn shutdown(mut self) {
        self.endpoint.close(0u32.into(), b"shutdown");
        if let Some(tx) = self.http_shutdown.take() {
            let _ = tx.send(());
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        for mut task in [self.http_task.take(), self.quic_task.take()].into_iter().flatten() {
            if tokio::time::timeout_at(deadline, &mut task).await.is_err() {
                tracing::warn!("Server graceful drain timed out; aborting its task");
                task.abort();
                let _ = task.await;
            }
        }
        let _ = tokio::time::timeout_at(deadline, self.endpoint.wait_idle()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ephemeral_config() -> ServerConfig {
        let mut config = ServerConfig::default();
        config.server.host = "127.0.0.1".to_string();
        config.server.http_port = 0;
        config.server.quic_port = 0;
        config.database.url = "sqlite::memory:".to_string();
        config.auth.jwt_secret = "launch-unit-test-secret-0123456789".to_string();
        config.server.tls_dir = std::env::temp_dir()
            .join(format!("finch-tls-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        config
    }

    /// After a server task exits on its own (`wait_for_exit` returned), `shutdown` must not panic.
    #[tokio::test]
    async fn shutdown_after_unexpected_exit_does_not_panic() {
        let mut server = launch(ephemeral_config()).await.unwrap();

        // Closing the endpoint ends the QUIC accept loop, like an unexpected exit.
        server.endpoint.close(0u32.into(), b"test");
        tokio::time::timeout(std::time::Duration::from_secs(5), server.wait_for_exit())
            .await
            .unwrap();

        let joined = tokio::spawn(server.shutdown()).await;
        assert!(joined.is_ok(), "shutdown panicked: {joined:?}");
    }

    #[tokio::test]
    async fn shutdown_bounds_a_stalled_graceful_drain() {
        let mut server = launch(ephemeral_config()).await.unwrap();
        server.http_task.take().unwrap().abort();
        server.http_task = Some(tokio::spawn(std::future::pending()));
        tokio::time::timeout(std::time::Duration::from_secs(6), server.shutdown())
            .await.expect("shutdown must stop even if HTTP graceful drain never finishes");
    }
}
