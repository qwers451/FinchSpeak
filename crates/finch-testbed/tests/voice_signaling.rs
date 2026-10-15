//! Voice signaling scenarios: presence, mute/deafen, SFU sender rewrite.

use std::time::Duration;

use finch_protocol::media::{MediaHeader, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_testbed::client::{audio_datagram, TestClient};
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn join_leave_presence() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let a_peer = a.session.session_peer_id;

    a.join_voice().await?;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::UserJoinedVoice { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;

    a.send(ClientMessage::LeaveVoice).await?;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::UserLeftVoice { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn muted_client_state_broadcast() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let a_peer = a.session.session_peer_id;
    a.join_voice().await?;
    b.join_voice().await?;

    a.send(ClientMessage::UpdateVoiceState { muted: true, deafened: false }).await?;
    let muted = b
        .expect_control(WAIT, |m| match m {
            ServerMessage::UserVoiceStateChanged { peer_id, muted, .. } if *peer_id == a_peer => {
                Some(*muted)
            }
            _ => None,
        })
        .await?;
    assert!(muted);
    Ok(())
}

#[tokio::test]
async fn deafened_client_receives_no_audio() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let mut c = TestClient::register(&server, "carol").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    c.join_voice().await?;

    let b_peer = b.session.session_peer_id;
    b.send(ClientMessage::UpdateVoiceState { muted: false, deafened: true }).await?;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::UserVoiceStateChanged { peer_id, deafened: true, .. } if *peer_id == b_peer => Some(()),
        _ => None,
    })
    .await?;
    b.drain_datagrams();
    c.drain_datagrams();

    let a_peer = a.session.session_peer_id;
    for seq in 0..50u16 {
        a.session.send_datagram(audio_datagram(voice_short, a_peer, seq, 60))?;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(b.drain_datagrams().is_empty(), "deafened client must not receive audio");
    assert!(c.drain_datagrams().len() >= 45, "listener must receive the audio");
    Ok(())
}

#[tokio::test]
async fn sender_peer_id_is_rewritten() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    b.drain_datagrams();

    a.session.send_datagram(audio_datagram(voice_short, 999_999, 1, 60))?;
    let packet = b.wait_datagram(WAIT).await?;
    let header = MediaHeader::from_slice(&packet[..MEDIA_HEADER_SIZE])?;
    assert_eq!(header.sender_peer_id, a.session.session_peer_id);
    Ok(())
}
