//! Custom join sounds: upload, validation, download, removal, broadcast.

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::ServerMessage;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

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
