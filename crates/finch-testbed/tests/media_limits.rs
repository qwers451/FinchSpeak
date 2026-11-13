//! Per-participant media limits in the SFU: a 7 Mbit/s screen share passes whole,
//! a 5 MB/s flood from one participant is capped and the lost datagrams are counted.

use std::sync::atomic::Ordering;
use std::time::Duration;

use finch_protocol::messages::ClientMessage;
use finch_protocol::models::VideoCodec;
use finch_screen::slice_p_frame;
use finch_server::health::TRAFFIC;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;
use tokio::time::Instant;

/// The media counters are process-wide; the scenarios run one after the other.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const FPS: u64 = 60;
const LIMIT_BYTES_PER_SEC: f64 = 1_500_000.0;
const LIMIT_BURST_BYTES: f64 = 262_144.0;

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
