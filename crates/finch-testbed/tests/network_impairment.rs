//! Voice over a lossy, jittery network (UDP impairment proxy between client B and the server).

use std::collections::HashSet;
use std::time::Duration;

use finch_audio::FRAME_SIZE_48K;
use finch_testbed::audio::{babble, chirp};
use finch_testbed::client::TestClient;
use finch_testbed::metrics::delay_samples;
use finch_testbed::netem::{NetemConfig, UdpImpairmentProxy};
use finch_testbed::server::TestServer;
use finch_testbed::voice::run_voice_link;
use tokio::net::UdpSocket;

const SR: usize = 48_000;
const MAX_BUFFER_PACKETS: usize = 6;

#[tokio::test]
async fn proxy_drops_and_delays() -> anyhow::Result<()> {
    let upstream = UdpSocket::bind("127.0.0.1:0").await?;
    let proxy = UdpImpairmentProxy::start(
        upstream.local_addr()?,
        NetemConfig { loss: 0.05, delay_ms: (0, 60), seed: 42 },
    )
    .await?;
    let sender = UdpSocket::bind("127.0.0.1:0").await?;

    for i in 0..1000u32 {
        sender.send_to(&i.to_be_bytes(), proxy.listen_addr).await?;
    }

    let mut received = Vec::new();
    let mut buf = [0u8; 16];
    while let Ok(Ok((n, _))) = tokio::time::timeout(Duration::from_millis(500), upstream.recv_from(&mut buf)).await {
        if n == 4 {
            received.push(u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]));
        }
    }

    assert!((930..=980).contains(&received.len()), "delivered {}", received.len());
    assert!(received.windows(2).any(|w| w[1] < w[0]), "expected some reordering");
    assert_eq!(received.iter().collect::<HashSet<_>>().len(), received.len(), "no duplicates");
    Ok(())
}

/// Longest run of all-zero 20 ms frames in `output[from..to]`.
fn longest_silent_run(output: &[f32], from: usize, to: usize) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for frame in output[from..to.min(output.len())].chunks(FRAME_SIZE_48K) {
        if frame.iter().all(|&s| s == 0.0) {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[tokio::test]
#[ignore = "AUD-022"]
async fn voice_under_loss_and_jitter() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let proxy = UdpImpairmentProxy::start(
        server.quic_addr,
        NetemConfig { loss: 0.05, delay_ms: (0, 60), seed: 42 },
    )
    .await?;

    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register_via(&server, "bob", proxy.listen_addr).await?;
    a.join_voice().await?;
    b.join_voice().await?;
    b.drain_datagrams();

    // Silence, a chirp for latency, then 5 s of continuous speech-like signal (no VAD pauses).
    let speech_start = SR / 5 + SR / 10;
    let mut input = vec![0.0f32; SR / 5];
    input.extend(chirp(SR / 10, 300.0, 3_000.0));
    input.extend(babble(5 * SR, -20.0, 3)?);

    let run = run_voice_link(&a, &mut b, &input, 15).await?;

    assert!(!run.received_headers.is_empty(), "receiver got no audio");
    let latency = delay_samples(&input[..speech_start], &run.output[..speech_start + SR], SR)
        .ok_or_else(|| anyhow::anyhow!("probe not found in receiver output"))?;
    let latency_ms = latency as f32 * 1000.0 / SR as f32;
    let longest_gap = longest_silent_run(
        &run.output,
        speech_start + latency + 2 * SR / 5,
        input.len() + latency - SR / 5,
    );
    eprintln!(
        "BASELINE netem: latency_ms = {latency_ms:.1}, max_buffered = {}, jitter_loss_rate = {:?}, delivered = {}/{} packets, longest_zero_run_frames = {longest_gap}",
        run.max_buffered_packets, run.loss_rate, run.received_headers.len(), run.sent_packets
    );

    assert!(latency_ms <= 200.0, "latency {latency_ms:.1} ms > 200 ms");
    assert!(
        run.max_buffered_packets <= MAX_BUFFER_PACKETS,
        "jitter buffer grew to {} packets",
        run.max_buffered_packets
    );
    assert!(run.loss_rate.unwrap_or(0.0) > 0.0, "losses must be observed");
    assert!(longest_gap <= 3, "{longest_gap} consecutive silent frames during speech");
    Ok(())
}
