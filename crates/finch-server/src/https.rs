//! HTTPS listener for the REST API.
//!
//! REST carries passwords and bearer tokens, so it is only served over TLS, with the same
//! self-signed certificate as QUIC: clients pin one fingerprint (TOFU) for both transports.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_rustls::TlsAcceptor;

/// A client that does not finish the TLS handshake within this time is dropped.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// In-flight requests get this long to finish on shutdown before connections are cut.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Build the rustls server configuration for the REST listener.
pub fn make_tls_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<rustls::ServerConfig>> {
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .context("Invalid TLS certificate or key for the REST listener")?;
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(tls))
}

/// Serve `app` over TLS until `shutdown` fires, then drain in-flight requests (bounded).
pub async fn serve(
    listener: TcpListener,
    tls: Arc<rustls::ServerConfig>,
    app: Router,
    mut shutdown: oneshot::Receiver<()>,
) {
    let acceptor = TlsAcceptor::from(tls);
    let graceful = GracefulShutdown::new();

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (tcp, peer) = match accepted {
                    Ok(accepted) => accepted,
                    Err(e) => {
                        tracing::warn!("REST accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let acceptor = acceptor.clone();
                let service = TowerToHyperService::new(app.clone());
                let watcher = graceful.watcher();

                tokio::spawn(async move {
                    let tls_stream = match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
                        Ok(Ok(stream)) => stream,
                        Ok(Err(e)) => {
                            tracing::debug!("REST TLS handshake from {peer} failed: {e}");
                            return;
                        }
                        Err(_) => {
                            tracing::debug!("REST TLS handshake from {peer} timed out");
                            return;
                        }
                    };
                    let builder = Builder::new(TokioExecutor::new());
                    let connection = builder.serve_connection_with_upgrades(TokioIo::new(tls_stream), service);
                    if let Err(e) = watcher.watch(connection).await {
                        tracing::debug!("REST connection from {peer} ended: {e}");
                    }
                });
            }
        }
    }

    drop(listener);
    if tokio::time::timeout(DRAIN_TIMEOUT, graceful.shutdown()).await.is_err() {
        tracing::warn!("REST graceful drain timed out; closing remaining connections");
    }
}
