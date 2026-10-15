//! Server liveness under a misbehaving client. The server runs on its own single-threaded
//! runtime in a separate OS thread, so a server deadlock fails the test instead of hanging it.

use std::net::SocketAddr;
use std::time::Duration;

use finch_client_core::api::ApiClient;
use finch_protocol::messages::ClientMessage;
use finch_server::config::ServerConfig;
use finch_server::launch::launch;
use finch_testbed::client::connect_to;

/// Start a server on a dedicated current-thread runtime; returns (http_url, quic_addr).
fn start_isolated_server(dir: &std::path::Path) -> anyhow::Result<(String, SocketAddr)> {
    let mut config = ServerConfig::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.http_port = 0;
    config.server.quic_port = 0;
    config.server.tls_dir = dir.join("tls").to_string_lossy().to_string();
    config.database.url = "sqlite::memory:".to_string();
    config.storage.upload_dir = dir.join("uploads").to_string_lossy().to_string();
    config.auth.jwt_secret = "liveness-test-secret-0123456789abcdef".to_string();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
            return;
        };
        rt.block_on(async move {
            match launch(config).await {
                Ok(server) => {
                    let _ = tx.send(Ok((format!("http://{}", server.http_addr), server.quic_addr)));
                    std::future::pending::<()>().await;
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });
    });
    rx.recv_timeout(Duration::from_secs(10))?
}

/// A client that floods pings and never reads the replies, then disconnects, must not
/// wedge the server (AUD-006: a DashMap guard held across `.await` in the Ping handler).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_survives_client_that_never_reads() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (http_url, quic_addr) = start_isolated_server(dir.path())?;

    let api = ApiClient::new(&http_url)?;
    let token = api.register("slowpoke", "password_123", "slowpoke").await?.token;
    let (session, events) = connect_to(quic_addr, &token).await?;

    // Never read `events`: the client stops draining the control stream once its queue
    // is full, so the server's Pong writer blocks on QUIC flow control.
    for i in 0..60_000u64 {
        if session.control_tx.send(ClientMessage::Ping { timestamp: i }).await.is_err() {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    session.connection.close(0u32.into(), b"bye");
    drop(events);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let check = async {
        let fresh = api.register("fresh", "password_123", "fresh").await?;
        connect_to(quic_addr, &fresh.token).await?;
        anyhow::Ok(())
    };
    tokio::time::timeout(Duration::from_secs(5), check)
        .await
        .map_err(|_| anyhow::anyhow!("server stopped responding after the slow client left"))??;
    Ok(())
}
