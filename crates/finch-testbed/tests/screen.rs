//! Screen share scenarios at protocol level: IDR over uni-streams, deltas over datagrams.

use std::time::{Duration, Instant};

use finch_client_core::session::SessionEvent;
use finch_protocol::media::{MediaHeader, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::VideoCodec;
use finch_screen::{slice_p_frame, FrameAssembler};
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

async fn streamer_and_viewer(server: &TestServer) -> anyhow::Result<(TestClient, TestClient, u32)> {
    let mut a = TestClient::register(server, "alice").await?;
    let mut b = TestClient::register(server, "bob").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;

    a.send(ClientMessage::StartScreenShare {
        codec: VideoCodec::H264,
        width: 1280,
        height: 720,
        fps: 30,
    })
    .await?;
    let a_peer = a.session.session_peer_id;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::ScreenShareStarted { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    b.send(ClientMessage::SubscribeScreenShare { target_user_id: a.session.user_id }).await?;
    // Subscribing has no acknowledgement; give the server a moment to register it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok((a, b, voice_short))
}

#[tokio::test]
async fn screen_share_keyframe_and_deltas() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;
    let a_peer = a.session.session_peer_id;

    let keyframe: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
    let key_header = MediaHeader::new_video_key(voice_short, a_peer, 0, 1000, 1, false);
    let mut uni = a.session.connection.open_uni().await?;
    uni.write_all(&key_header.to_bytes()).await?;
    uni.write_all(&keyframe).await?;
    uni.finish()?;

    let (header, payload) = loop {
        if let SessionEvent::UniStream { header, payload } = b.next_event(WAIT).await? {
            break (header, payload);
        }
    };
    assert_eq!(payload.as_ref(), keyframe.as_slice());

    let mut assembler = FrameAssembler::new();
    assembler.on_keyframe_received(header.frame_id, header.timestamp, payload);

    let delta: Vec<u8> = (0..5000).map(|i| (i % 199) as u8).collect();
    for packet in slice_p_frame(voice_short, a_peer, 1, 2, 1033, &delta, false) {
        a.session.send_datagram(packet.encode())?;
    }

    let mut assembled = None;
    while assembled.is_none() {
        let dgram = b.wait_datagram(WAIT).await?;
        let hdr = MediaHeader::from_slice(&dgram[..MEDIA_HEADER_SIZE])?;
        assembled = assembler.push_fragment(&hdr, dgram.slice(MEDIA_HEADER_SIZE..), Instant::now());
    }
    let frame = assembled.unwrap_or_else(|| unreachable!());
    assert_eq!(frame.data.as_ref(), delta.as_slice());
    Ok(())
}

#[tokio::test]
async fn keyframe_request_forwarded() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (mut a, b, _) = streamer_and_viewer(&server).await?;

    b.send(ClientMessage::RequestKeyframe { target_peer_id: a.session.session_peer_id }).await?;
    a.expect_control(Duration::from_secs(1), |m| match m {
        ServerMessage::KeyframeRequested => Some(()),
        _ => None,
    })
    .await?;
    Ok(())
}

/// Only members of the streamer's voice room may watch (AUD-017).
#[tokio::test]
async fn outsider_cannot_subscribe_to_screen() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (a, _b, voice_short) = streamer_and_viewer(&server).await?;
    let mut outsider = TestClient::register(&server, "mallory").await?;
    outsider
        .send(ClientMessage::SubscribeScreenShare { target_user_id: a.session.user_id })
        .await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    outsider.drain_datagrams();

    let delta: Vec<u8> = vec![1u8; 3000];
    for packet in slice_p_frame(voice_short, a.session.session_peer_id, 1, 2, 1033, &delta, false) {
        a.session.send_datagram(packet.encode())?;
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(outsider.drain_datagrams().is_empty(), "outsider received the screen stream");
    Ok(())
}

async fn send_keyframe(a: &TestClient, voice_short: u32, frame_id: u16, size: usize) -> anyhow::Result<()> {
    let header = MediaHeader::new_video_key(voice_short, a.session.session_peer_id, 0, 1000, frame_id, false);
    let mut uni = a.session.connection.open_uni().await?;
    uni.write_all(&header.to_bytes()).await?;
    uni.write_all(&vec![(frame_id % 251) as u8; size]).await?;
    uni.finish()?;
    Ok(())
}

async fn next_keyframe(viewer: &mut TestClient, timeout: Duration) -> anyhow::Result<(MediaHeader, usize)> {
    loop {
        if let SessionEvent::UniStream { header, payload } = viewer.next_event(timeout).await? {
            return Ok((header, payload.len()));
        }
    }
}

/// A 1080p IDR is several MB; the server must forward it (AUD-035: it used to cut at 1 MB while
/// the client accepts 10 MB).
#[tokio::test]
async fn large_keyframe_is_forwarded() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;

    send_keyframe(&a, voice_short, 1, 3 * 1024 * 1024).await?;
    let (header, len) = next_keyframe(&mut b, Duration::from_secs(5)).await?;
    assert_eq!((header.frame_id, len), (1, 3 * 1024 * 1024));
    Ok(())
}

/// Keyframe streams are read independently: one that never finishes must not hold up the next
/// (AUD-035: they used to be read one at a time).
#[tokio::test]
async fn stalled_keyframe_stream_does_not_block_the_next() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;

    let stalled_header = MediaHeader::new_video_key(voice_short, a.session.session_peer_id, 0, 1000, 1, false);
    let mut stalled = a.session.connection.open_uni().await?;
    stalled.write_all(&stalled_header.to_bytes()).await?;
    stalled.write_all(&[1u8; 100]).await?; // never finished

    send_keyframe(&a, voice_short, 2, 64 * 1024).await?;
    let (header, len) = next_keyframe(&mut b, WAIT).await?;
    assert_eq!((header.frame_id, len), (2, 64 * 1024));
    drop(stalled);
    Ok(())
}
