//! "Позвать стаю": who receives a flock call, what is refused, and what a legacy client sees.
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use dashmap::DashMap;
use finch_client_core::tofu::create_client_config;
use finch_protocol::codec::CodecError;
use finch_protocol::models::ChannelType;
use finch_protocol::{ClientMessage, ServerMessage, decode_message, encode_message};
use finch_testbed::client::{TEST_PASSWORD, TestClient};
use finch_testbed::server::TestServer;
use serde::Serialize;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(3);
const QUIET: Duration = Duration::from_millis(300);

/// A call as received: (from_user_id, from_name, channel_id).
type Call = (Uuid, String, Uuid);

async fn call(client: &mut TestClient) -> anyhow::Result<Call> {
    client.expect_control(WAIT, |message| match message {
        ServerMessage::FlockCall { from_user_id, from_name, channel_id } => Some((*from_user_id, from_name.clone(), *channel_id)),
        _ => None,
    }).await
}

async fn refusal(client: &mut TestClient) -> anyhow::Result<u16> {
    client.expect_control(WAIT, |message| match message {
        ServerMessage::Error { code, .. } => Some(*code),
        _ => None,
    }).await
}

async fn no_call(client: &mut TestClient) -> anyhow::Result<()> {
    client.expect_no_control(QUIET, |message| matches!(message, ServerMessage::FlockCall { .. })).await
}

async fn barrier(client: &mut TestClient) -> anyhow::Result<()> {
    client.send(ClientMessage::Ping { timestamp: 7 }).await?;
    client.expect_control(WAIT, |message| matches!(message, ServerMessage::Pong { timestamp: 7 }).then_some(())).await
}

/// The boss (first account, so admin) creates a second voice room for the tests.
async fn lounge(boss: &TestClient) -> anyhow::Result<Uuid> {
    Ok(boss.api.create_channel("Lounge", "voice", None, None).await?.id.parse()?)
}

#[tokio::test]
async fn flock_call_reaches_the_room_but_not_the_caller() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let mut carol = TestClient::register(&server, "carol").await?;
    let room = lounge(&boss).await?;
    alice.join_voice().await?;
    bob.join_voice().await?;
    alice.send(ClientMessage::CallFlock { target_channel: room }).await?;

    assert_eq!(call(&mut bob).await?, (alice.session.user_id, "alice".into(), room));
    no_call(&mut carol).await?;
    no_call(&mut alice).await?;
    Ok(())
}

#[tokio::test]
async fn call_user_reaches_only_the_person_called() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let mut carol = TestClient::register(&server, "carol").await?;
    alice.join_voice().await?;
    let room = alice.channel(ChannelType::Voice).0;

    alice.send(ClientMessage::CallUser { user_id: bob.session.user_id }).await?;
    assert_eq!(call(&mut bob).await?, (alice.session.user_id, "alice".into(), room));
    no_call(&mut carol).await?;
    Ok(())
}

#[tokio::test]
async fn second_call_within_ten_seconds_is_refused_and_not_sent() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let mut carol = TestClient::register(&server, "carol").await?;
    let room = lounge(&boss).await?;
    alice.join_voice().await?;
    bob.join_voice().await?;
    alice.send(ClientMessage::CallFlock { target_channel: room }).await?;
    call(&mut bob).await?;

    alice.send(ClientMessage::CallUser { user_id: carol.session.user_id }).await?;
    assert_eq!(refusal(&mut alice).await?, 429);
    no_call(&mut carol).await?;
    Ok(())
}

#[tokio::test]
async fn invalid_targets_are_refused_and_do_not_use_up_the_budget() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let room = lounge(&boss).await?;
    let (text_room, _) = boss.channel(ChannelType::Text);
    alice.join_voice().await?;
    bob.join_voice().await?;

    alice.send(ClientMessage::CallFlock { target_channel: text_room }).await?;
    assert_eq!(refusal(&mut alice).await?, 400);
    alice.send(ClientMessage::CallFlock { target_channel: Uuid::new_v4() }).await?;
    assert_eq!(refusal(&mut alice).await?, 400);
    alice.send(ClientMessage::CallUser { user_id: Uuid::new_v4() }).await?;
    assert_eq!(refusal(&mut alice).await?, 404);
    alice.send(ClientMessage::CallUser { user_id: bob.session.user_id }).await?;
    assert_eq!(refusal(&mut alice).await?, 400);
    no_call(&mut bob).await?;

    alice.send(ClientMessage::CallFlock { target_channel: room }).await?;
    assert_eq!(call(&mut bob).await?.2, room);
    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum Level150Message {
    Authenticate { token: String, supports_voice_extensions: bool, client_level: u16 },
}

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
async fn a_level_150_client_in_the_room_never_receives_a_flock_call() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let room = lounge(&boss).await?;
    let old = server.api()?.register("oldie", TEST_PASSWORD, "Oldie").await?;
    let pins = Arc::new(DashMap::new());
    pins.insert("localhost".into(), server.certificate_fingerprint.clone());
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    endpoint.set_default_client_config(create_client_config("localhost", pins)?);
    let connection = endpoint.connect(server.quic_addr, "localhost")?.await?;
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(&encode_message(&Level150Message::Authenticate {
        token: old.token,
        supports_voice_extensions: true,
        client_level: finch_protocol::CLIENT_LEVEL_1_5,
    })?).await?;
    let mut buffered = BytesMut::new();
    assert!(matches!(read_control(&mut recv, &mut buffered).await?, ServerMessage::Authenticated { .. }));
    let (home, _) = alice.join_voice().await?;
    send.write_all(&encode_message(&ClientMessage::JoinVoice { channel_id: home })?).await?;
    send.write_all(&encode_message(&ClientMessage::Ping { timestamp: 9 })?).await?;
    loop {
        if let ServerMessage::Pong { timestamp: 9 } = read_control(&mut recv, &mut buffered).await? { break; }
    }

    alice.send(ClientMessage::CallFlock { target_channel: room }).await?;
    barrier(&mut alice).await?;
    send.write_all(&encode_message(&ClientMessage::Ping { timestamp: 10 })?).await?;
    loop {
        match read_control(&mut recv, &mut buffered).await? {
            ServerMessage::FlockCall { .. } => anyhow::bail!("level 150 client received a flock call"),
            ServerMessage::Pong { timestamp: 10 } => break,
            _ => (),
        }
    }
    Ok(())
}
