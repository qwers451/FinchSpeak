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
fn start_isolated_server(dir: &std::path::Path) -> anyhow::Result<(ApiClient, SocketAddr)> {
    let mut config = ServerConfig::default();
    config.auth.registration = finch_server::config::RegistrationMode::Open;
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
                    let _ = tx.send(Ok((format!("https://{}", server.http_addr), server.certificate_fingerprint.clone(), server.quic_addr)));
                    std::future::pending::<()>().await;
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });
    });
    let (url, fingerprint, quic_addr) = rx.recv_timeout(Duration::from_secs(10))??;
    let pins = std::sync::Arc::new(dashmap::DashMap::new());
    pins.insert("127.0.0.1".to_string(), fingerprint);
    Ok((ApiClient::with_tofu(&url, pins)?, quic_addr))
}

/// A client that floods pings and never reads the replies, then disconnects, must not
/// wedge the server (AUD-006: a DashMap guard held across `.await` in the Ping handler).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_survives_client_that_never_reads() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (api, quic_addr) = start_isolated_server(dir.path())?;

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

/// One client with a full control queue must not delay broadcasts to everyone else
/// (AUD-007: `broadcast_all` awaited each client in turn).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_client_does_not_block_chat_for_others() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (api, quic_addr) = start_isolated_server(dir.path())?;

    let stalled = api.register("stalled", "password_123", "stalled").await?.token;
    let (stalled_session, _stalled_events) = connect_to(quic_addr, &stalled).await?;
    for i in 0..60_000u64 {
        if stalled_session.control_tx.send(ClientMessage::Ping { timestamp: i }).await.is_err() {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    let alice = api.register("alice", "password_123", "alice").await?.token;
    let bob = api.register("bob", "password_123", "bob").await?.token;
    let (a, _a_events) = connect_to(quic_addr, &alice).await?;
    let (_b, mut b_events) = connect_to(quic_addr, &bob).await?;
    let text_id = a
        .initial_state
        .channels
        .iter()
        .find(|c| c.channel_type == finch_protocol::models::ChannelType::Text)
        .map(|c| c.id)
        .ok_or_else(|| anyhow::anyhow!("no text channel"))?;

    a.control_tx
        .send(ClientMessage::SendMessage {
            client_msg_id: None,
            channel_id: text_id,
            content: "still alive".to_string(),
            attachments: vec![],
        })
        .await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let event = tokio::time::timeout_at(deadline, b_events.recv())
            .await
            .map_err(|_| anyhow::anyhow!("bob did not get the message while another client was stalled"))?
            .ok_or_else(|| anyhow::anyhow!("bob's session closed"))?;
        if let finch_client_core::session::SessionEvent::Control(
            finch_protocol::messages::ServerMessage::NewMessage { content, .. },
        ) = event
            && content == "still alive"
        {
            return Ok(());
        }
    }
}
