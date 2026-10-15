//! Resilience scenarios: server restart, abrupt client loss, failed handshakes.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use finch_client_core::tofu::TofuVerifier;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::{ChannelType, UserStatus};
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;
use quinn::crypto::rustls::QuicClientConfig;

const WAIT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn client_reconnects_after_server_restart() -> anyhow::Result<()> {
    let mut server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let (text_id, _) = a.channel(ChannelType::Text);

    a.send(ClientMessage::SendMessage {
            client_msg_id: None,
        channel_id: text_id,
        content: "before restart".to_string(),
        attachments: vec![],
    })
    .await?;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::NewMessage { content, .. } if content == "before restart" => Some(()),
        _ => None,
    })
    .await?;

    server.restart().await?;
    // The client has no automatic reconnect (AUD-025); the bench reconnects explicitly.
    a.reconnect(&server).await?;
    b.reconnect(&server).await?;

    let history = a.api.fetch_messages(text_id, None, 50).await?;
    assert!(history.messages.iter().any(|m| m.content == "before restart"));

    a.join_voice().await?;
    let a_peer = a.session.session_peer_id;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::UserJoinedVoice { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn abrupt_disconnect_cleans_presence() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    a.join_voice().await?;
    b.join_voice().await?;

    let a_peer = a.session.session_peer_id;
    a.session.connection.close(0u32.into(), b"gone");
    b.expect_control(Duration::from_secs(20), |m| match m {
        ServerMessage::UserLeftVoice { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn server_responsive_after_abrupt_disconnect() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    a.join_voice().await?;
    b.join_voice().await?;

    let a_user = a.session.user_id;
    a.session.connection.close(0u32.into(), b"gone");
    b.expect_control(Duration::from_secs(20), |m| match m {
        ServerMessage::PresenceUpdate { user_id, status: UserStatus::Offline } if *user_id == a_user => {
            Some(())
        }
        _ => None,
    })
    .await?;

    let mut c = TestClient::register(&server, "carol").await?;
    c.join_voice().await?;
    Ok(())
}

#[tokio::test]
async fn server_survives_failed_handshake() -> anyhow::Result<()> {
    let server = TestServer::start().await?;

    // 1. Garbage UDP datagram shaped like a QUIC Initial.
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    let mut garbage = vec![0xC3u8; 1200];
    garbage[1..5].copy_from_slice(&1u32.to_be_bytes());
    socket.send_to(&garbage, server.quic_addr).await?;

    // 2. Real QUIC client offering an ALPN the server does not speak.
    let verifier = Arc::new(TofuVerifier::new("localhost".to_string(), Arc::new(DashMap::new())));
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"wrong".to_vec()];
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls)?)));
    let attempt = endpoint.connect(server.quic_addr, "localhost")?;
    let _ = tokio::time::timeout(Duration::from_secs(2), attempt).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 3. A normal client must still get in.
    tokio::time::timeout(Duration::from_secs(5), TestClient::register(&server, "alice")).await??;
    Ok(())
}

/// While the server is down the client keeps retrying and gets back in once it returns (AUD-025).
#[tokio::test]
async fn client_reconnects_automatically_when_server_returns() -> anyhow::Result<()> {
    use finch_client_core::session::{reconnect_with_backoff, Backoff, ConnectParams};

    let mut server = TestServer::start().await?;
    let a = TestClient::register(&server, "alice").await?;
    let token = a.api.token().unwrap_or_default().to_string();
    let quic_addr = server.quic_addr;

    server.stop().await;
    let params = ConnectParams {
        quic_addr,
        hostname: "localhost".to_string(),
        token,
        require_known_fingerprint: false,
    };
    let backoff = Backoff { initial: Duration::from_millis(200), max: Duration::from_millis(800) };
    let reconnecting = tokio::spawn(reconnect_with_backoff(params, Arc::new(DashMap::new()), backoff));

    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!reconnecting.is_finished(), "must keep retrying while the server is down");
    server.restart().await?;

    let (session, _events) = tokio::time::timeout(Duration::from_secs(5), reconnecting).await???;
    assert_ne!(session.session_peer_id, 0);
    Ok(())
}

/// Retrying cannot fix a rejected token; reconnecting gives up immediately (AUD-025).
#[tokio::test]
async fn reconnect_stops_on_rejected_token() -> anyhow::Result<()> {
    use finch_client_core::error::ClientError;
    use finch_client_core::session::{reconnect_with_backoff, Backoff, ConnectParams};

    let server = TestServer::start().await?;
    let params = ConnectParams {
        quic_addr: server.quic_addr,
        hostname: "localhost".to_string(),
        token: "garbage".to_string(),
        require_known_fingerprint: false,
    };
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        reconnect_with_backoff(params, Arc::new(DashMap::new()), Backoff::default()),
    )
    .await?;
    assert!(matches!(result, Err(ClientError::AuthRejected { .. })), "got {:?}", result.map(|_| ()));
    Ok(())
}
