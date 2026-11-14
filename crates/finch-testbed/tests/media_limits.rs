//! Per-participant media limits in the SFU: a 7 Mbit/s screen share passes whole,
//! a 5 MB/s flood from one participant is capped and the lost datagrams are counted.

use std::sync::atomic::Ordering;
use std::time::Duration;

use finch_client_core::session::SessionEvent;
use finch_protocol::media::{MediaHeader, MAX_KEYFRAME_BYTES, MEDIA_HEADER_SIZE};
use finch_protocol::messages::ClientMessage;
use finch_protocol::models::VideoCodec;
use finch_screen::slice_p_frame;
use finch_server::health::TRAFFIC;
use finch_testbed::client::{audio_datagram, connect_with_token, TestClient};
use finch_testbed::server::TestServer;
use tokio::time::Instant;

/// The media counters are process-wide; the scenarios run one after the other.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const FPS: u64 = 60;
const LIMIT_BYTES_PER_SEC: f64 = 1_500_000.0;
const LIMIT_BURST_BYTES: f64 = 262_144.0;
const LIMIT_KEYFRAME_BYTES_PER_SEC: f64 = 1_000_000.0;

fn dropped() -> u64 {
    TRAFFIC.datagrams_dropped.load(Ordering::Relaxed)
}

async fn streamer_and_viewer(server: &TestServer) -> anyhow::Result<(TestClient, TestClient, u32)> {
    let mut a = TestClient::register(server, "alice").await?;
    let mut b = TestClient::register(server, "bob").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    a.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 1920, height: 1080, fps: 60 }).await?;
    let a_peer = a.session.session_peer_id;
    b.expect_control(Duration::from_secs(2), |m| match m {
        finch_protocol::messages::ServerMessage::ScreenShareStarted { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    b.send(ClientMessage::SubscribeScreenShare { target_user_id: a.session.user_id }).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok((a, b, voice_short))
}

#[derive(Debug, Default)]
struct Outcome {
    sent_packets: usize,
    send_errors: usize,
    received_packets: usize,
    received_bytes: u64,
    elapsed: Duration,
}

/// Send `seconds` of delta frames of `frame_bytes` each at 60 FPS, real time, while `receiver` drains.
async fn stream(
    sender: &TestClient,
    receiver: &mut TestClient,
    voice_short: u32,
    frame_bytes: usize,
    seconds: u64,
) -> Outcome {
    let sender_peer = sender.session.session_peer_id;
    let frame: Vec<u8> = (0..frame_bytes).map(|i| (i % 251) as u8).collect();
    let mut outcome = Outcome::default();
    let started = Instant::now();
    let frames = seconds * FPS;
    for index in 0..frames {
        let due = started + Duration::from_micros(index * 1_000_000 / FPS);
        // Keep draining while waiting for the next frame, so the receiver never backs up.
        while Instant::now() < due {
            let datagrams = receiver.drain_datagrams();
            outcome.received_packets += datagrams.len();
            outcome.received_bytes += datagrams.iter().map(|d| d.len() as u64).sum::<u64>();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let timestamp = (index * 1000 / FPS) as u32;
        for packet in slice_p_frame(voice_short, sender_peer, (index * 64) as u16, index as u16, timestamp, &frame, false) {
            match sender.session.send_datagram(packet.encode()) {
                Ok(()) => outcome.sent_packets += 1,
                Err(_) => outcome.send_errors += 1,
            }
        }
    }
    // Let the last frame arrive.
    let deadline = Instant::now() + Duration::from_secs(2);
    while outcome.received_packets < outcome.sent_packets && Instant::now() < deadline {
        let datagrams = receiver.drain_datagrams();
        outcome.received_packets += datagrams.len();
        outcome.received_bytes += datagrams.iter().map(|d| d.len() as u64).sum::<u64>();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    outcome.elapsed = started.elapsed();
    outcome
}

#[tokio::test]
async fn screen_share_at_7_mbit_per_s_passes_without_loss() -> anyhow::Result<()> {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;
    // 7 Mbit/s at 60 FPS is about 14.6 KB per frame.
    let frame_bytes = 14_583;
    let dropped_before = dropped();

    let outcome = stream(&a, &mut b, voice_short, frame_bytes, 2).await;

    assert_eq!(outcome.send_errors, 0, "the client could not queue its own datagrams");
    assert!(outcome.sent_packets > 0);
    assert_eq!(outcome.received_packets, outcome.sent_packets, "a 7 Mbit/s screen share lost datagrams");
    assert_eq!(dropped(), dropped_before, "the media limit dropped a legitimate screen share");
    Ok(())
}

#[tokio::test]
async fn flood_of_5_mb_per_s_is_capped_and_counted() -> anyhow::Result<()> {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;
    // 5 MB/s at 60 FPS is about 83 KB per frame.
    let frame_bytes = 83_334;
    let dropped_before = dropped();

    let outcome = stream(&a, &mut b, voice_short, frame_bytes, 2).await;

    let elapsed = outcome.elapsed.as_secs_f64();
    let cap = LIMIT_BYTES_PER_SEC * elapsed + LIMIT_BURST_BYTES + 64.0 * 1024.0;
    assert!(
        (outcome.received_bytes as f64) <= cap,
        "receiver got {} bytes in {elapsed:.2} s, the limit allows about {cap:.0}",
        outcome.received_bytes
    );
    assert!(outcome.received_packets < outcome.sent_packets, "no datagram was refused");
    assert!(dropped() > dropped_before, "refused datagrams are not counted in datagrams_dropped");
    Ok(())
}

/// Keyframes received on uni-streams since the last call.
async fn keyframe_bytes(receiver: &mut TestClient) -> u64 {
    let mut bytes = 0;
    while let Ok(event) = receiver.next_event(Duration::from_millis(1)).await {
        if let SessionEvent::UniStream { payload, .. } = event {
            bytes += (payload.len() + MEDIA_HEADER_SIZE) as u64;
        }
    }
    bytes
}

#[tokio::test]
async fn keyframe_flood_on_uni_streams_is_capped_like_datagrams() -> anyhow::Result<()> {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;
    let a_peer = a.session.session_peer_id;
    let keyframe = vec![0x5a_u8; 1024 * 1024];
    let dropped_before = dropped();

    // 5 MB/s of 1 MB keyframes for 6 s: more than the keyframe burst can hold.
    let started = Instant::now();
    let mut sent_bytes = 0u64;
    let mut received_bytes = 0u64;
    let mut index = 0u64;
    while started.elapsed() < Duration::from_secs(6) {
        let target = started.elapsed().as_secs_f64() * 5_000_000.0;
        while (sent_bytes as f64) < target {
            let header = MediaHeader::new_video_key(voice_short, a_peer, index as u16, (index * 33) as u32, index as u16, false);
            a.session.send_keyframe(&header, &keyframe).await?;
            sent_bytes += (keyframe.len() + MEDIA_HEADER_SIZE) as u64;
            index += 1;
        }
        received_bytes += keyframe_bytes(&mut b).await;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    received_bytes += keyframe_bytes(&mut b).await;
    let elapsed = started.elapsed().as_secs_f64();

    // Keyframe rate plus one maximum keyframe as burst, with a little slack for the last keyframes.
    let cap = LIMIT_KEYFRAME_BYTES_PER_SEC * elapsed + MAX_KEYFRAME_BYTES as f64 + 2.0 * (keyframe.len() + MEDIA_HEADER_SIZE) as f64;
    assert!(
        (received_bytes as f64) <= cap,
        "receiver got {received_bytes} bytes of keyframes in {elapsed:.2} s, the limit allows about {cap:.0}"
    );
    assert!(dropped() > dropped_before, "refused keyframes are not counted in datagrams_dropped");
    Ok(())
}

/// What a receiver got in one pump: datagrams (voice and screen deltas) and keyframe streams.
#[derive(Default)]
struct Received {
    datagrams: usize,
    keyframe_bytes: u64,
}

/// Collect every event until the receiver is idle for `idle`.
async fn pump(receiver: &mut TestClient, idle: Duration, into: &mut Received) {
    while let Ok(event) = receiver.next_event(idle).await {
        match event {
            SessionEvent::Datagram(_) => into.datagrams += 1,
            SessionEvent::UniStream { payload, .. } => into.keyframe_bytes += (payload.len() + MEDIA_HEADER_SIZE) as u64,
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_3_mb_keyframe_does_not_stall_the_datagrams_after_it() -> anyhow::Result<()> {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;
    let a_peer = a.session.session_peer_id;
    let dropped_before = dropped();

    // A 3 MB keyframe, then voice and screen deltas straight after it.
    let keyframe = vec![0x33_u8; 3 * 1024 * 1024];
    a.session.send_keyframe(&MediaHeader::new_video_key(voice_short, a_peer, 0, 1000, 7, false), &keyframe).await?;
    let mut sent = 0usize;
    for seq in 0..20u16 {
        a.session.send_datagram(audio_datagram(voice_short, a_peer, seq, 100))?;
        sent += 1;
    }
    for packet in slice_p_frame(voice_short, a_peer, 100, 8, 2000, &vec![0x44_u8; 6000], false) {
        a.session.send_datagram(packet.encode())?;
        sent += 1;
    }

    let mut received = Received::default();
    pump(&mut b, Duration::from_secs(1), &mut received).await;
    assert_eq!(received.keyframe_bytes, (keyframe.len() + MEDIA_HEADER_SIZE) as u64, "the keyframe was not forwarded");
    assert_eq!(received.datagrams, sent, "datagrams after a large keyframe were lost");
    assert_eq!(dropped(), dropped_before, "the keyframe delayed or dropped the datagrams");
    Ok(())
}

#[tokio::test]
async fn a_1_mb_keyframe_is_admitted_while_a_7_mbit_share_runs() -> anyhow::Result<()> {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start().await?;
    let (a, mut b, voice_short) = streamer_and_viewer(&server).await?;
    let a_peer = a.session.session_peer_id;
    let frame: Vec<u8> = (0..14_583).map(|i| (i % 251) as u8).collect();
    let keyframe = vec![0x55_u8; 1024 * 1024];
    let dropped_before = dropped();

    // 2 s of 7 Mbit/s screen deltas at 60 FPS, with a 1 MB keyframe in the middle.
    let started = Instant::now();
    let mut sent = 0usize;
    let mut received = Received::default();
    for index in 0..120u64 {
        let due = started + Duration::from_micros(index * 1_000_000 / FPS);
        while Instant::now() < due {
            pump(&mut b, Duration::from_millis(1), &mut received).await;
        }
        if index == 60 {
            let header = MediaHeader::new_video_key(voice_short, a_peer, 0, 1000, 9, false);
            a.session.send_keyframe(&header, &keyframe).await?;
        }
        let timestamp = (index * 1000 / FPS) as u32;
        for packet in slice_p_frame(voice_short, a_peer, (index * 64) as u16, index as u16, timestamp, &frame, false) {
            a.session.send_datagram(packet.encode())?;
            sent += 1;
        }
    }
    pump(&mut b, Duration::from_secs(1), &mut received).await;
    assert!(
        received.keyframe_bytes >= (keyframe.len() + MEDIA_HEADER_SIZE) as u64,
        "the 1 MB keyframe was refused during the share (got {} bytes)",
        received.keyframe_bytes
    );
    assert_eq!(received.datagrams, sent, "the 7 Mbit/s share lost datagrams around the keyframe");
    assert_eq!(dropped(), dropped_before, "the keyframe or the share was dropped");
    Ok(())
}

#[tokio::test]
async fn two_sessions_of_one_user_share_one_limit() -> anyhow::Result<()> {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (channel_id, voice_short) = alice.join_voice().await?;
    bob.join_voice().await?;

    // A second session of the same user, in the same voice channel.
    let token = alice.api.token().ok_or_else(|| anyhow::anyhow!("alice has no token"))?;
    let (second, _events) = connect_with_token(&server, token).await?;
    second
        .control_tx
        .send(ClientMessage::JoinVoice { channel_id })
        .await
        .map_err(|_| anyhow::anyhow!("control channel closed"))?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let senders = [alice.session.session_peer_id, second.session_peer_id];
    let dropped_before = dropped();
    // Each session sends 2.5 MB/s of 1020-byte audio datagrams: 5 MB/s from one user.
    let started = Instant::now();
    let mut sent = [0u64; 2];
    let mut seq = [0u16; 2];
    let mut received_bytes = 0u64;
    while started.elapsed() < Duration::from_secs(2) {
        let per_session_target = started.elapsed().as_secs_f64() * 2_500_000.0 / 1020.0;
        for (slot, &peer) in senders.iter().enumerate() {
            let session = if slot == 0 { &alice.session } else { &second };
            while (sent[slot] as f64) < per_session_target {
                let datagram = audio_datagram(voice_short, peer, seq[slot], 1000);
                let _ = session.send_datagram(datagram);
                sent[slot] += 1;
                seq[slot] = seq[slot].wrapping_add(1);
            }
        }
        for datagram in bob.drain_datagrams() {
            received_bytes += datagram.len() as u64;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        for datagram in bob.drain_datagrams() {
            received_bytes += datagram.len() as u64;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let elapsed = started.elapsed().as_secs_f64();

    // One user's budget: about 1.5 MB/s in total, not 1.5 MB/s per session.
    let cap = LIMIT_BYTES_PER_SEC * elapsed + LIMIT_BURST_BYTES + 64.0 * 1024.0;
    assert!(
        (received_bytes as f64) <= cap,
        "bob got {received_bytes} bytes from one user's two sessions in {elapsed:.2} s; the shared limit allows about {cap:.0}"
    );
    assert!(dropped() > dropped_before, "the shared budget refused nothing");
    Ok(())
}
