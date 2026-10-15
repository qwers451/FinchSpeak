//! finch-client-core against an in-process finch-server.

use std::sync::Arc;
use std::time::Duration;

use bytes::{BufMut, BytesMut};
use dashmap::DashMap;
use finch_client_core::api::ApiClient;
use finch_client_core::error::ClientError;
use finch_client_core::session::{connect, ConnectParams, Session, SessionEvent, EVENT_QUEUE_CAPACITY};
use finch_protocol::media::MediaHeader;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_server::config::ServerConfig;
use finch_server::launch::{launch, RunningServer};
use tokio::sync::mpsc;
use uuid::Uuid;

async fn start_server() -> anyhow::Result<RunningServer> {
    let mut config = ServerConfig::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.http_port = 0;
    config.server.quic_port = 0;
    config.database.url = "sqlite::memory:".to_string();
    config.storage.upload_dir = std::env::temp_dir()
        .join(format!("finch-client-core-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    config.auth.jwt_secret = "client-core-test-secret-0123456789".to_string();
    config.server.tls_dir = std::env::temp_dir()
        .join(format!("finch-client-core-tls-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    launch(config).await
}

fn api_for(server: &RunningServer) -> anyhow::Result<ApiClient> {
    Ok(ApiClient::new(&format!("http://{}", server.http_addr))?)
}

async fn connect_with(
    server: &RunningServer,
    token: &str,
) -> Result<(Session, mpsc::Receiver<SessionEvent>), ClientError> {
    connect(
        ConnectParams {
            quic_addr: server.quic_addr,
            hostname: "localhost".to_string(),
            token: token.to_string(),
        },
        Arc::new(DashMap::new()),
    )
    .await
}

async fn wait_control<T>(
    events: &mut mpsc::Receiver<SessionEvent>,
    mut pred: impl FnMut(&ServerMessage) -> Option<T>,
) -> anyhow::Result<T> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("event stream closed"))?;
        if let SessionEvent::Control(msg) = event
            && let Some(found) = pred(&msg)
        {
            return Ok(found);
        }
    }
}

fn channel_of(session: &Session, kind: ChannelType) -> (Uuid, u32) {
    let ch = session
        .initial_state
        .channels
        .iter()
        .find(|c| c.channel_type == kind)
        .expect("default channels are seeded");
    (ch.id, ch.short_id)
}

#[tokio::test]
async fn register_login_and_connect() -> anyhow::Result<()> {
    let server = start_server().await?;
    let api = api_for(&server)?;

    let auth = api.register("alice", "password_123", "Alice").await?;
    assert!(!auth.token.is_empty());
    assert_eq!(auth.user.display_name, "Alice");

    let (session, _events) = connect_with(&server, &auth.token).await?;
    assert_ne!(session.session_peer_id, 0);
    assert!(!session.initial_state.channels.is_empty());

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn bad_password_returns_401() -> anyhow::Result<()> {
    let server = start_server().await?;
    let api = api_for(&server)?;
    api.register("alice", "password_123", "Alice").await?;

    match api.login("alice", "wrong_password").await {
        Err(ClientError::Http { status, .. }) => assert_eq!(status, 401),
        other => panic!("expected HTTP 401, got {other:?}"),
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
#[ignore = "AUD-047"]
async fn invalid_token_rejected_on_quic() -> anyhow::Result<()> {
    let server = start_server().await?;

    match connect_with(&server, "garbage").await {
        Err(ClientError::AuthRejected { code, .. }) => assert_eq!(code, 401),
        Err(other) => panic!("expected AuthRejected, got {other:?}"),
        Ok(_) => panic!("garbage token must be rejected"),
    }

    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn control_events_survive_datagram_flood() -> anyhow::Result<()> {
    let server = start_server().await?;
    let api = api_for(&server)?;
    let alice = api.register("alice", "password_123", "Alice").await?;
    let bob = api.register("bob", "password_123", "Bob").await?;

    let (a, mut a_events) = connect_with(&server, &alice.token).await?;
    let (b, mut b_events) = connect_with(&server, &bob.token).await?;
    let (voice_id, voice_short) = channel_of(&a, ChannelType::Voice);
    let (text_id, _) = channel_of(&a, ChannelType::Text);

    b.control_tx.send(ClientMessage::JoinVoice { channel_id: voice_id }).await?;
    a.control_tx.send(ClientMessage::JoinVoice { channel_id: voice_id }).await?;
    let a_peer = a.session_peer_id;
    wait_control(&mut a_events, |m| match m {
        ServerMessage::UserJoinedVoice { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    wait_control(&mut b_events, |m| match m {
        ServerMessage::UserJoinedVoice { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;

    // B stops reading while A floods it with audio datagrams.
    for seq in 0..5000u16 {
        let header = MediaHeader::new_audio(voice_short, a_peer, seq, seq as u32 * 20, false, false);
        let mut packet = BytesMut::new();
        header.write_to(&mut packet);
        packet.put_slice(&[0u8; 40]);
        let _ = a.send_datagram(packet.freeze());
        if seq % 500 == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    a.control_tx
        .send(ClientMessage::SendMessage {
            channel_id: text_id,
            content: "after flood".to_string(),
            attachments: vec![],
        })
        .await?;

    // Drain B: count queued datagrams until the control message shows up.
    let mut datagrams_before_message = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let event = tokio::time::timeout_at(deadline, b_events.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("event stream closed"))?;
        match event {
            SessionEvent::Datagram(_) => datagrams_before_message += 1,
            SessionEvent::Control(ServerMessage::NewMessage { content, .. }) if content == "after flood" => break,
            _ => {}
        }
    }

    // The queue filled up (so datagrams were dropped, not blocked on) and the
    // control message still arrived.
    assert!(
        datagrams_before_message >= EVENT_QUEUE_CAPACITY / 2,
        "queue never filled: only {datagrams_before_message} datagrams queued"
    );
    assert!(
        datagrams_before_message <= EVENT_QUEUE_CAPACITY + 16,
        "{datagrams_before_message} datagrams queued, more than the queue holds"
    );

    server.shutdown().await;
    Ok(())
}
