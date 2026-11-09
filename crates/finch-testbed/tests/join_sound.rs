//! Custom join sounds: upload, validation, download, removal, broadcast.

use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use dashmap::DashMap;
use finch_client_core::error::ClientError;
use finch_client_core::tofu::create_client_config;
use finch_protocol::codec::{decode_message, encode_message, CodecError};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::{TestClient, TEST_PASSWORD};
use finch_testbed::server::TestServer;
use serde::Serialize;

const WAIT: Duration = Duration::from_secs(3);

fn http_status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

fn wav(rate: u32, frames: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(out.len() + data_len as usize, 0);
    out
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum LegacyClientMessage {
    Authenticate { token: String },
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
            let size = recv.read(&mut chunk).await?.ok_or_else(|| anyhow::anyhow!("control stream closed"))?;
            buffered.extend_from_slice(&chunk[..size]);
        }
    }).await?
}

#[tokio::test]
async fn legacy_authentication_does_not_enable_new_voice_events() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let bob = server.api()?.register("bob", TEST_PASSWORD, "Bob").await?;
    let pins = Arc::new(DashMap::new());
    pins.insert("localhost".into(), server.certificate_fingerprint.clone());
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    endpoint.set_default_client_config(create_client_config("localhost", pins)?);
    let connection = tokio::time::timeout(WAIT, endpoint.connect(server.quic_addr, "localhost")?).await??;
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(&encode_message(&LegacyClientMessage::Authenticate { token: bob.token })?).await?;

    let mut buffered = BytesMut::new();
    let ServerMessage::Authenticated { user_id, session_peer_id, .. } = read_control(&mut recv, &mut buffered).await? else {
        anyhow::bail!("legacy client could not authenticate");
    };
    let (room, _) = alice.channel(ChannelType::Voice);
    send.write_all(&encode_message(&ClientMessage::JoinVoice { channel_id: room })?).await?;
    while !matches!(read_control(&mut recv, &mut buffered).await?, ServerMessage::UserJoinedVoice { peer_id, .. } if peer_id == session_peer_id) {}

    alice.api.upload_join_sound(wav(24000, 12000)).await?;
    let lounge = alice.api.create_channel("Lounge", "voice", None, None).await?;
    let move_error = alice.api.move_voice(user_id, session_peer_id, lounge.id.parse()?).await.expect_err("legacy client cannot receive a move");
    assert_eq!(http_status(&move_error), Some(400));

    send.write_all(&encode_message(&ClientMessage::Ping { timestamp: 42 })?).await?;
    loop {
        match read_control(&mut recv, &mut buffered).await? {
            ServerMessage::Pong { timestamp: 42 } => break,
            ServerMessage::JoinSoundChanged { .. } | ServerMessage::VoiceMoved { .. } => {
                anyhow::bail!("a legacy client received an unknown voice extension");
            }
            _ => (),
        }
    }
    Ok(())
}

#[tokio::test]
async fn a_join_sound_is_stored_announced_and_removed() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;

    let missing = bob.api.download_join_sound_cached(alice.session.user_id, None).await.expect_err("none yet");
    assert_eq!(http_status(&missing), Some(404));

    let sound = wav(24000, 12000);
    alice.api.upload_join_sound(sound.clone()).await?;
    let told = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::JoinSoundChanged { user_id } => Some(*user_id),
            _ => None,
        })
        .await?;
    assert_eq!(told, alice.session.user_id);

    let (bytes, etag) = bob.api.download_join_sound_cached(alice.session.user_id, None).await?;
    assert_eq!(bytes.as_deref(), Some(sound.as_slice()));
    let etag = etag.expect("etag");
    let (again, _) = bob.api.download_join_sound_cached(alice.session.user_id, Some(&etag)).await?;
    assert!(again.is_none(), "304 with a current ETag");

    alice.api.delete_join_sound().await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::JoinSoundChanged { .. }).then_some(())).await?;
    let gone = bob.api.download_join_sound_cached(alice.session.user_id, None).await.expect_err("removed");
    assert_eq!(http_status(&gone), Some(404));
    Ok(())
}

#[tokio::test]
async fn long_oversized_and_non_wav_sounds_are_refused() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let too_long = alice.api.upload_join_sound(wav(24000, 24000 * 3)).await.expect_err("3 s");
    assert_eq!(http_status(&too_long), Some(400));
    let junk = alice.api.upload_join_sound(b"this is not a wav".to_vec()).await.expect_err("junk");
    assert_eq!(http_status(&junk), Some(400));
    let huge = alice.api.upload_join_sound(vec![0u8; 700 * 1024]).await.expect_err("700 KB");
    assert_eq!(http_status(&huge), Some(413));
    alice.api.upload_join_sound(wav(24000, 24000 * 2)).await?;
    Ok(())
}

#[tokio::test]
async fn a_delayed_legacy_file_cleanup_cannot_remove_a_replacement_sound() -> anyhow::Result<()> {
    let storage = tempfile::tempdir()?;
    let server = TestServer::start_with(|config| {
        config.storage.upload_dir = storage.path().to_string_lossy().into_owned();
    }).await?;
    let alice = TestClient::register(&server, "alice").await?;
    let old_directory = storage.path().join("join_sounds");
    let old_path = old_directory.join(format!("{}.wav", alice.session.user_id));
    std::fs::create_dir(&old_directory)?;
    std::fs::write(&old_path, wav(24000, 12000))?;

    let replacement = wav(24000, 6000);
    alice.api.upload_join_sound(replacement.clone()).await?;
    // Simulate an earlier delete reaching the legacy path after the replacement upload.
    match std::fs::remove_file(old_path) {
        Ok(()) => {},
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {},
        Err(err) => return Err(err.into()),
    }

    let (bytes, _) = alice.api.download_join_sound_cached(alice.session.user_id, None).await?;
    assert_eq!(bytes.as_deref(), Some(replacement.as_slice()));
    Ok(())
}
