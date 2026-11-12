//! Activity privacy, device ownership and wire compatibility.
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use dashmap::DashMap;
use finch_client_core::tofu::create_client_config;
use finch_protocol::{ClientMessage, ServerMessage, decode_message, encode_message};
use finch_protocol::codec::CodecError;
use finch_testbed::client::{TestClient, TEST_PASSWORD, connect_with_token};
use finch_testbed::server::TestServer;
use serde::Serialize;

const WAIT: Duration = Duration::from_secs(3);

async fn activity(client: &mut TestClient, user: uuid::Uuid) -> anyhow::Result<Option<String>> {
    client.expect_control(WAIT, |message| match message {
        ServerMessage::ActivityChanged { user_id, game } if *user_id == user => Some(game.clone()),
        _ => None,
    }).await
}

async fn barrier(client: &mut TestClient) -> anyhow::Result<()> {
    client.send(ClientMessage::Ping { timestamp: 42 }).await?;
    client.expect_control(WAIT, |message| matches!(message, ServerMessage::Pong { timestamp: 42 }).then_some(())).await
}

#[tokio::test]
async fn activity_is_broadcast_snapshotted_rate_limited_and_cleared_immediately() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let user = alice.session.user_id;
    alice.send(ClientMessage::SetActivity { game: Some("я".repeat(80)) }).await?;
    assert_eq!(activity(&mut bob, user).await?, Some("я".repeat(64)));
    barrier(&mut alice).await?;
    let snapshot = alice.api.server_state().await?;
    assert!(snapshot.features.contains(&"activity".into()));
    assert_eq!(snapshot.activities.iter().find(|activity| activity.user_id == user).unwrap().game, "я".repeat(64));
    let charlie = TestClient::register(&server, "charlie").await?;
    assert_eq!(charlie.session.initial_state.activities.iter().find(|activity| activity.user_id == user).unwrap().game, "я".repeat(64));

    alice.send(ClientMessage::SetActivity { game: Some("Too soon".into()) }).await?;
    barrier(&mut alice).await?;
    assert_eq!(alice.api.server_state().await?.activities.iter().find(|activity| activity.user_id == user).unwrap().game, "я".repeat(64));
    alice.send(ClientMessage::SetActivity { game: Some("  ".into()) }).await?;
    assert_eq!(activity(&mut bob, user).await?, None);
    barrier(&mut alice).await?;
    assert!(alice.api.server_state().await?.activities.is_empty());
    Ok(())
}

#[tokio::test]
async fn activity_belongs_to_the_last_reporting_device_and_does_not_fall_back_on_disconnect() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let user = alice.session.user_id;
    alice.send(ClientMessage::SetActivity { game: Some("Portal".into()) }).await?;
    assert_eq!(activity(&mut bob, user).await?, Some("Portal".into()));
    let (second, _events) = connect_with_token(&server, alice.api.token().unwrap()).await?;
    second.control_tx.send(ClientMessage::SetActivity { game: Some("Hades".into()) }).await?;
    assert_eq!(activity(&mut bob, user).await?, Some("Hades".into()));
    alice.send(ClientMessage::SetActivity { game: None }).await?;
    barrier(&mut alice).await?;
    assert_eq!(alice.api.server_state().await?.activities[0].game, "Hades");
    second.connection.close(0u32.into(), b"test disconnect");
    assert_eq!(activity(&mut bob, user).await?, None);
    assert!(bob.api.server_state().await?.activities.is_empty());
    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum LegacyMessage { Authenticate { token: String } }

async fn read_control(recv: &mut quinn::RecvStream, buffered: &mut BytesMut) -> anyhow::Result<ServerMessage> {
    tokio::time::timeout(WAIT, async {
        let mut chunk = [0; 4096];
        loop {
            match decode_message(buffered) {
                Ok(message) => return Ok(message),
                Err(CodecError::Incomplete { .. }) => (),
                Err(error) => return Err(error.into()),
            }
            let size = recv.read(&mut chunk).await?.ok_or_else(|| anyhow::anyhow!("control closed"))?;
            buffered.extend_from_slice(&chunk[..size]);
        }
    }).await?
}

#[tokio::test]
async fn activity_never_reaches_a_legacy_control_stream() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let bob = server.api()?.register("bob", TEST_PASSWORD, "Bob").await?;
    let pins = Arc::new(DashMap::new());
    pins.insert("localhost".into(), server.certificate_fingerprint.clone());
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    endpoint.set_default_client_config(create_client_config("localhost", pins)?);
    let connection = endpoint.connect(server.quic_addr, "localhost")?.await?;
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(&encode_message(&LegacyMessage::Authenticate { token: bob.token })?).await?;
    let mut buffered = BytesMut::new();
    assert!(matches!(read_control(&mut recv, &mut buffered).await?, ServerMessage::Authenticated { .. }));
    alice.send(ClientMessage::SetActivity { game: Some("Portal".into()) }).await?;
    barrier(&mut alice).await?;
    send.write_all(&encode_message(&ClientMessage::Ping { timestamp: 42 })?).await?;
    loop {
        match read_control(&mut recv, &mut buffered).await? {
            ServerMessage::ActivityChanged { .. } => anyhow::bail!("legacy client received an unknown activity variant"),
            ServerMessage::Pong { timestamp: 42 } => break,
            _ => (),
        }
    }
    Ok(())
}
