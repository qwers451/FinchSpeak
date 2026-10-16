//! Control stream robustness against a misbehaving server (AUD-052).

use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use dashmap::DashMap;
use finch_client_core::session::{connect, ConnectParams, SessionEvent};
use finch_protocol::codec::{decode_message, encode_message};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ServerState;

/// After authenticating, the server sends a frame that is not valid MessagePack.
async fn run_corrupting_server(endpoint: quinn::Endpoint) -> anyhow::Result<()> {
    let conn = endpoint
        .accept()
        .await
        .ok_or_else(|| anyhow::anyhow!("no incoming connection"))?
        .await?;
    let (mut send, mut recv) = conn.accept_bi().await?;

    let mut buf = BytesMut::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = recv.read(&mut chunk).await?.ok_or_else(|| anyhow::anyhow!("closed"))?;
        buf.extend_from_slice(&chunk[..n]);
        if let Ok(ClientMessage::Authenticate { .. }) = decode_message::<ClientMessage>(&mut buf) {
            break;
        }
    }

    let ack = ServerMessage::Authenticated {
        user_id: uuid::Uuid::new_v4(),
        session_peer_id: 1001,
        channels: vec![],
        initial_state: ServerState { channels: vec![], online_users: vec![], voice_states: vec![], unread: vec![] },
    };
    send.write_all(&encode_message(&ack)?).await?;
    // Length prefix 4, then 4 bytes that are not a valid ServerMessage.
    send.write_all(&[0, 0, 0, 4, 0xc1, 0xc1, 0xc1, 0xc1]).await?;
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok(())
}

#[tokio::test]
async fn corrupt_control_frame_ends_the_session_visibly() -> anyhow::Result<()> {
    let endpoint = quinn::Endpoint::server(finch_server::quic::make_server_config()?, "127.0.0.1:0".parse()?)?;
    let addr = endpoint.local_addr()?;
    tokio::spawn(run_corrupting_server(endpoint));

    let (_session, mut events) = connect(
        ConnectParams {
            quic_addr: addr,
            hostname: "localhost".to_string(),
            token: "any".to_string(),
            require_known_fingerprint: false,
        },
        Arc::new(DashMap::new()),
    )
    .await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Some(SessionEvent::ConnectionLost(_))) => return Ok(()),
            Ok(Some(_)) => continue,
            Ok(None) => anyhow::bail!("event stream ended without ConnectionLost"),
            Err(_) => anyhow::bail!("client silently stalled on a corrupt control frame"),
        }
    }
}
