//! Screen share end to end (AUD-018): a real sharer thread encodes frames, the server forwards
//! them, and a late-joining viewer asks for a key frame and decodes the stream.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use finch_client_core::screen::{FrameSource, ScreenSharer, ScreenViewer, ShareConfig};
use finch_client_core::session::SessionEvent;
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::VideoCodec;
use finch_screen::{PixelFormat, RawFrame};
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

/// Moving gradient; never two equal frames.
struct Animation(u32);

impl FrameSource for Animation {
    fn next_frame(&mut self) -> Result<RawFrame, String> {
        self.0 += 1;
        let mut data = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let v = ((x + y + self.0 * 4) % 256) as u8;
                data.extend_from_slice(&[v, 255 - v, 100, 255]);
            }
        }
        Ok(RawFrame::new(WIDTH, HEIGHT, PixelFormat::Bgra, Bytes::from(data)))
    }
}

/// A source that stops delivering frames.
struct Broken;

impl FrameSource for Broken {
    fn next_frame(&mut self) -> Result<RawFrame, String> {
        Err("window closed".into())
    }
}

async fn join_voice_pair(server: &TestServer) -> anyhow::Result<(TestClient, TestClient, u32)> {
    let mut a = TestClient::register(server, "alice").await?;
    let mut b = TestClient::register(server, "bob").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    Ok((a, b, voice_short))
}

#[tokio::test(flavor = "multi_thread")]
async fn late_viewer_gets_a_keyframe_on_request_and_decodes_the_stream() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (mut a, mut b, voice_short) = join_voice_pair(&server).await?;
    let a_peer = a.session.session_peer_id;
    let a_user = a.session.user_id;

    a.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: WIDTH, height: HEIGHT, fps: 30 })
        .await?;
    let sharer = Arc::new(ScreenSharer::start(
        a.session.connection.clone(),
        tokio::runtime::Handle::current(),
        voice_short,
        a_peer,
        ShareConfig { fps: 30, bitrate_bps: None },
        Box::new(Animation(0)),
        |_| {},
    ));

    // The sharer's side of the control stream: forward key frame requests to the encoder.
    let key_requests = {
        let sharer = sharer.clone();
        tokio::spawn(async move {
            let mut seen = 0u32;
            while let Ok(event) = a.next_event(Duration::from_secs(20)).await {
                if matches!(event, SessionEvent::Control(ServerMessage::KeyframeRequested)) {
                    seen += 1;
                    sharer.request_keyframe();
                }
            }
            seen
        })
    };

    // Bob subscribes late, so he starts in the middle of the stream.
    tokio::time::sleep(Duration::from_millis(600)).await;
    b.send(ClientMessage::SubscribeScreenShare { target_user_id: a_user }).await?;

    let mut viewer = ScreenViewer::new();
    let mut decoder = Decoder::new()?;
    let (mut decoded, mut first_was_key) = (0, None);
    let deadline = Instant::now() + Duration::from_secs(15);
    while decoded < 20 && Instant::now() < deadline {
        let now = Instant::now();
        if viewer.take_keyframe_request(now) {
            b.send(ClientMessage::RequestKeyframe { target_peer_id: a_peer }).await?;
        }
        viewer.tick(now);
        let frames = match b.next_event(Duration::from_millis(50)).await {
            Ok(SessionEvent::Datagram(bytes)) => {
                let header = MediaHeader::from_slice(&bytes[..MEDIA_HEADER_SIZE])?;
                if header.media_type != MediaType::VideoDelta {
                    continue;
                }
                viewer.on_delta_fragment(&header, bytes.slice(MEDIA_HEADER_SIZE..), now).into_iter().collect()
            }
            Ok(SessionEvent::UniStreamStarted { .. }) => {
                viewer.keyframe_stream_started(now);
                Vec::new()
            }
            Ok(SessionEvent::UniStream { header, payload }) => vec![viewer.on_keyframe(&header, payload, now)],
            _ => Vec::new(),
        };
        for frame in frames {
            let picture = decoder.decode(&frame.data)?.ok_or_else(|| anyhow::anyhow!("no picture"))?;
            assert_eq!(picture.dimensions(), (WIDTH as usize, HEIGHT as usize));
            first_was_key.get_or_insert(frame.is_keyframe);
            decoded += 1;
        }
    }

    assert!(decoded >= 20, "decoded only {decoded} frames");
    assert_eq!(first_was_key, Some(true), "a late viewer must start on a key frame, not on a delta");

    key_requests.abort();
    let _ = key_requests.await;
    let mut sharer = Arc::try_unwrap(sharer).map_err(|_| anyhow::anyhow!("sharer still shared"))?;
    sharer.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn sharer_reports_when_capture_keeps_failing() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (a, _b, voice_short) = join_voice_pair(&server).await?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let sharer = ScreenSharer::start(
        a.session.connection.clone(),
        tokio::runtime::Handle::current(),
        voice_short,
        a.session.session_peer_id,
        ShareConfig { fps: 30, bitrate_bps: None },
        Box::new(Broken),
        move |reason| {
            let _ = tx.send(reason);
        },
    );
    let reason = tokio::time::timeout(Duration::from_secs(10), rx).await??;
    assert!(reason.contains("window closed"), "{reason}");
    assert!(!sharer.is_running());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_the_sharer_ends_the_capture_thread_without_a_report() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (a, _b, voice_short) = join_voice_pair(&server).await?;
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let mut sharer = ScreenSharer::start(
        a.session.connection.clone(),
        tokio::runtime::Handle::current(),
        voice_short,
        a.session.session_peer_id,
        ShareConfig { fps: 30, bitrate_bps: None },
        Box::new(Animation(0)),
        move |reason| {
            let _ = tx.send(reason);
        },
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(sharer.is_running());
    sharer.stop();
    assert!(!sharer.is_running());
    assert!(rx.try_recv().is_err(), "a requested stop is not an error");
    Ok(())
}
