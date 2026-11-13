//! Soundboard: upload rules, the board's limit, who may delete, who hears a played sound, and the 3-second budget.
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use dashmap::DashMap;
use finch_client_core::error::ClientError;
use finch_client_core::tofu::create_client_config;
use finch_protocol::codec::CodecError;
use finch_protocol::{ClientMessage, ServerMessage, decode_message, encode_message};
use finch_testbed::client::{TEST_PASSWORD, TestClient};
use finch_testbed::server::TestServer;
use serde::Serialize;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(3);
const QUIET: Duration = Duration::from_millis(300);

/// A 16-bit PCM WAV with the given format and length.
fn wav(rate: u32, channels: u16, bits: u16, ms: u32) -> Vec<u8> {
    let frames = rate * ms / 1000;
    let data_len = frames * u32::from(channels) * u32::from(bits / 8);
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
    out.extend_from_slice(&(channels * (bits / 8)).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(out.len() + data_len as usize, 0);
    out
}

/// A sound the board accepts.
fn good_wav() -> Vec<u8> {
    wav(24_000, 1, 16, 1000)
}

fn http_status(error: &ClientError) -> u16 {
    match error {
        ClientError::Http { status, .. } => *status,
        other => panic!("expected an HTTP error, got {other:?}"),
    }
}

async fn played(client: &mut TestClient) -> anyhow::Result<(Uuid, Uuid)> {
    client.expect_control(WAIT, |message| match message {
        ServerMessage::SoundPlayed { sound_id, user_id } => Some((*sound_id, *user_id)),
        _ => None,
    }).await
}

async fn no_sound(client: &mut TestClient) -> anyhow::Result<()> {
    client.expect_no_control(QUIET, |message| matches!(message, ServerMessage::SoundPlayed { .. })).await
}

async fn refusal(client: &mut TestClient) -> anyhow::Result<u16> {
    client.expect_control(WAIT, |message| match message {
        ServerMessage::Error { code, .. } => Some(*code),
        _ => None,
    }).await
}

async fn barrier(client: &mut TestClient) -> anyhow::Result<()> {
    client.send(ClientMessage::Ping { timestamp: 7 }).await?;
    client.expect_control(WAIT, |message| matches!(message, ServerMessage::Pong { timestamp: 7 }).then_some(())).await
}

#[tokio::test]
async fn an_uploaded_sound_is_listed_and_downloadable() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let data = good_wav();

    let sound = alice.api.upload_board_sound(data.clone(), "Гудок", Some("📯")).await?;
    assert_eq!(sound.name, "Гудок");
    assert_eq!(sound.emoji.as_deref(), Some("📯"));
    assert_eq!(sound.uploader_id, alice.session.user_id);
    assert_eq!(sound.duration_ms, 1000);

    let listed = alice.api.soundboard().await?;
    assert_eq!(listed, vec![sound.clone()]);
    let (bytes, _etag) = alice.api.download_board_sound_cached(sound.id, None).await?;
    assert_eq!(bytes.as_deref(), Some(data.as_slice()));
    Ok(())
}

#[tokio::test]
async fn only_mono_24_khz_16_bit_sounds_of_at_most_five_seconds_are_accepted() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;

    let refused = [
        (wav(24_000, 2, 16, 500), "Стерео"),
        (wav(22_050, 1, 16, 500), "Частота"),
        (wav(24_000, 1, 8, 500), "Восемь бит"),
        (wav(24_000, 1, 16, 5200), "Длинный"),
    ];
    for (data, name) in refused {
        let error = alice.api.upload_board_sound(data, name, None).await.unwrap_err();
        assert_eq!(http_status(&error), 400, "{name} should be refused");
    }
    let mut oversized = good_wav();
    oversized.resize(300 * 1024, 0); // past the 256 KB limit
    let error = alice.api.upload_board_sound(oversized, "Большой", None).await.unwrap_err();
    assert_eq!(http_status(&error), 413);
    let error = alice.api.upload_board_sound(good_wav(), "", None).await.unwrap_err();
    assert_eq!(http_status(&error), 400);
    let error = alice.api.upload_board_sound(good_wav(), &"я".repeat(33), None).await.unwrap_err();
    assert_eq!(http_status(&error), 400);
    assert!(alice.api.soundboard().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_full_board_refuses_the_next_sound() -> anyhow::Result<()> {
    let server = TestServer::start_with(|config| config.soundboard.max_sounds = 2).await?;
    let alice = TestClient::register(&server, "alice").await?;
    alice.api.upload_board_sound(good_wav(), "Раз", None).await?;
    alice.api.upload_board_sound(good_wav(), "Два", None).await?;
    let error = alice.api.upload_board_sound(good_wav(), "Три", None).await.unwrap_err();
    assert_eq!(http_status(&error), 400);
    assert_eq!(alice.api.soundboard().await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn only_the_uploader_or_a_moderator_may_delete() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let alice = TestClient::register(&server, "alice").await?;
    let bob = TestClient::register(&server, "bob").await?;
    let mine = alice.api.upload_board_sound(good_wav(), "Мой", None).await?;
    let theirs = boss.api.upload_board_sound(good_wav(), "Босса", None).await?;

    let error = bob.api.delete_board_sound(mine.id).await.unwrap_err();
    assert_eq!(http_status(&error), 403);
    alice.api.delete_board_sound(mine.id).await?;
    boss.api.delete_board_sound(theirs.id).await?;
    assert!(alice.api.soundboard().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_played_sound_reaches_the_room_including_the_player_only() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let mut carol = TestClient::register(&server, "carol").await?;
    let sound = alice.api.upload_board_sound(good_wav(), "Гонг", None).await?;
    alice.join_voice().await?;
    bob.join_voice().await?;

    alice.send(ClientMessage::PlaySound { sound_id: sound.id }).await?;
    let expected = (sound.id, alice.session.user_id);
    assert_eq!(played(&mut bob).await?, expected);
    assert_eq!(played(&mut alice).await?, expected);
    no_sound(&mut carol).await?;
    Ok(())
}

#[tokio::test]
async fn playing_outside_a_room_or_an_unknown_sound_is_refused() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let sound = alice.api.upload_board_sound(good_wav(), "Гонг", None).await?;

    alice.send(ClientMessage::PlaySound { sound_id: sound.id }).await?;
    assert_eq!(refusal(&mut alice).await?, 400);
    alice.join_voice().await?;
    alice.send(ClientMessage::PlaySound { sound_id: Uuid::new_v4() }).await?;
    assert_eq!(refusal(&mut alice).await?, 404);
    Ok(())
}

#[tokio::test]
async fn a_second_sound_within_three_seconds_is_refused_and_not_sent() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let first = alice.api.upload_board_sound(good_wav(), "Раз", None).await?;
    let second = alice.api.upload_board_sound(good_wav(), "Два", None).await?;
    alice.join_voice().await?;
    bob.join_voice().await?;

    alice.send(ClientMessage::PlaySound { sound_id: first.id }).await?;
    assert_eq!(played(&mut bob).await?.0, first.id);
    alice.send(ClientMessage::PlaySound { sound_id: second.id }).await?;
    assert_eq!(refusal(&mut alice).await?, 429);
    no_sound(&mut bob).await?;

    tokio::time::sleep(Duration::from_millis(3100)).await;
    alice.send(ClientMessage::PlaySound { sound_id: second.id }).await?;
    assert_eq!(played(&mut bob).await?.0, second.id);
    Ok(())
}

#[tokio::test]
async fn a_muted_person_cannot_play_a_sound() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let sound = boss.api.upload_board_sound(good_wav(), "Гонг", None).await?;
    alice.join_voice().await?;
    boss.api.moderate_voice(alice.session.user_id, "mute").await?;

    alice.send(ClientMessage::PlaySound { sound_id: sound.id }).await?;
    assert_eq!(refusal(&mut alice).await?, 403);
    Ok(())
}

#[tokio::test]
async fn a_disabled_soundboard_refuses_its_routes() -> anyhow::Result<()> {
    let server = TestServer::start_with(|config| config.soundboard.enabled = false).await?;
    let alice = TestClient::register(&server, "alice").await?;
    let error = alice.api.soundboard().await.unwrap_err();
    assert_eq!(http_status(&error), 404);
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
async fn a_level_150_client_in_the_room_never_receives_a_sound() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let sound = alice.api.upload_board_sound(good_wav(), "Гонг", None).await?;
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

    alice.send(ClientMessage::PlaySound { sound_id: sound.id }).await?;
    barrier(&mut alice).await?;
    send.write_all(&encode_message(&ClientMessage::Ping { timestamp: 10 })?).await?;
    loop {
        match read_control(&mut recv, &mut buffered).await? {
            ServerMessage::SoundPlayed { .. } => anyhow::bail!("level 150 client received a played sound"),
            ServerMessage::Pong { timestamp: 10 } => break,
            _ => (),
        }
    }
    Ok(())
}
