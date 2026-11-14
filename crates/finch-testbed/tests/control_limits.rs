//! Control messages that make other people work are limited: keyframe requests (only from
//! viewers, two per second per pair) and join / voice state / screen share (ten per ten seconds).

use std::time::{Duration, Instant};

use finch_client_core::session::SessionEvent;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::VideoCodec;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

/// Alice shares her screen, Bob watches it, Carol is in the room but does not watch.
async fn sharing_room(server: &TestServer) -> anyhow::Result<(TestClient, TestClient, TestClient)> {
    let mut a = TestClient::register(server, "alice").await?;
    let mut b = TestClient::register(server, "bob").await?;
    let mut c = TestClient::register(server, "carol").await?;
    a.join_voice().await?;
    b.join_voice().await?;
    c.join_voice().await?;
    a.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 1280, height: 720, fps: 30 }).await?;
    let a_peer = a.session.session_peer_id;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::ScreenShareStarted { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    b.send(ClientMessage::SubscribeScreenShare { target_user_id: a.session.user_id }).await?;
    // Subscribing has no acknowledgement; give the server a moment to register it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok((a, b, c))
}

/// Keyframe requests that reach `a` within `within`.
async fn keyframe_requests_within(a: &mut TestClient, within: Duration) -> usize {
    let deadline = Instant::now() + within;
    let mut seen = 0;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return seen;
        }
        match a.next_event(left).await {
            Ok(SessionEvent::Control(ServerMessage::KeyframeRequested)) => seen += 1,
            Ok(_) => {}
            Err(_) => return seen,
        }
    }
}

fn is_rate_limited(m: &ServerMessage) -> bool {
    matches!(m, ServerMessage::Error { code: 429, .. })
}

async fn expect_rate_limited(client: &mut TestClient) -> anyhow::Result<()> {
    client
        .expect_control(WAIT, |m| if is_rate_limited(m) { Some(()) } else { None })
        .await
}

#[tokio::test]
async fn keyframe_request_from_a_non_viewer_is_ignored() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (mut a, _b, c) = sharing_room(&server).await?;

    c.send(ClientMessage::RequestKeyframe { target_peer_id: a.session.session_peer_id }).await?;
    assert_eq!(keyframe_requests_within(&mut a, Duration::from_millis(500)).await, 0);
    Ok(())
}

#[tokio::test]
async fn viewer_keyframe_request_is_forwarded_immediately() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (mut a, b, _c) = sharing_room(&server).await?;

    b.send(ClientMessage::RequestKeyframe { target_peer_id: a.session.session_peer_id }).await?;
    assert_eq!(keyframe_requests_within(&mut a, Duration::from_millis(300)).await, 1);
    Ok(())
}

#[tokio::test]
async fn keyframe_burst_is_capped_at_two_per_second() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (mut a, b, _c) = sharing_room(&server).await?;

    for _ in 0..5 {
        b.send(ClientMessage::RequestKeyframe { target_peer_id: a.session.session_peer_id }).await?;
    }
    assert_eq!(keyframe_requests_within(&mut a, Duration::from_millis(500)).await, 2);
    Ok(())
}

#[tokio::test]
async fn keyframe_retry_after_half_a_second_is_forwarded() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (mut a, b, _c) = sharing_room(&server).await?;
    let a_peer = a.session.session_peer_id;

    b.send(ClientMessage::RequestKeyframe { target_peer_id: a_peer }).await?;
    assert_eq!(keyframe_requests_within(&mut a, Duration::from_millis(500)).await, 1);
    b.send(ClientMessage::RequestKeyframe { target_peer_id: a_peer }).await?;
    assert_eq!(keyframe_requests_within(&mut a, Duration::from_millis(500)).await, 1);
    Ok(())
}

#[tokio::test]
async fn join_voice_is_limited_to_ten_per_ten_seconds() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel_id, _) = alice.join_voice().await?; // the first of ten

    for _ in 0..9 {
        alice.send(ClientMessage::JoinVoice { channel_id }).await?;
    }
    alice
        .expect_no_control(Duration::from_millis(300), is_rate_limited)
        .await?;
    alice.send(ClientMessage::JoinVoice { channel_id }).await?;
    expect_rate_limited(&mut alice).await
}

#[tokio::test]
async fn voice_state_updates_are_limited_to_ten_per_ten_seconds() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    alice.join_voice().await?;

    for i in 0..10 {
        alice.send(ClientMessage::UpdateVoiceState { muted: i % 2 == 0, deafened: false }).await?;
    }
    alice
        .expect_no_control(Duration::from_millis(300), is_rate_limited)
        .await?;
    alice.send(ClientMessage::UpdateVoiceState { muted: true, deafened: false }).await?;
    expect_rate_limited(&mut alice).await
}

#[tokio::test]
async fn screen_share_starts_are_limited_to_ten_per_ten_seconds() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    alice.join_voice().await?;
    let start = || ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 1280, height: 720, fps: 30 };

    for _ in 0..10 {
        alice.send(start()).await?;
    }
    alice
        .expect_no_control(Duration::from_millis(300), is_rate_limited)
        .await?;
    alice.send(start()).await?;
    expect_rate_limited(&mut alice).await
}

/// The normal flow (join, mute, unmute, share, watch, ask for a keyframe) is not refused.
#[tokio::test]
async fn normal_voice_and_screen_flow_is_not_limited() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (mut a, mut b, _c) = sharing_room(&server).await?;
    let a_peer = a.session.session_peer_id;

    a.send(ClientMessage::UpdateVoiceState { muted: true, deafened: false }).await?;
    a.send(ClientMessage::UpdateVoiceState { muted: false, deafened: false }).await?;
    b.send(ClientMessage::RequestKeyframe { target_peer_id: a_peer }).await?;
    a.expect_control(WAIT, |m| matches!(m, ServerMessage::KeyframeRequested).then_some(())).await?;

    a.expect_no_control(Duration::from_millis(200), is_rate_limited).await?;
    b.expect_no_control(Duration::from_millis(200), is_rate_limited).await?;
    Ok(())
}
