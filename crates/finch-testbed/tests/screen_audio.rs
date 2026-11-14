//! Sound of a shared screen (game, video): sent as `ScreenAudio` datagrams to the viewers and
//! played through its own mixer channel.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use finch_audio::{screen_audio_key, AudioMixer, FRAME_SIZE_48K};
use finch_client_core::screen::ScreenAudioForwarder;
use finch_client_core::session::SessionEvent;
use finch_protocol::media::{MediaHeader, MediaPacket, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::VideoCodec;
use finch_screen::{slice_p_frame, ScreenAudioEncoder, STEREO_FRAME_SAMPLES};
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

/// 20 ms of a stereo tone: left 440 Hz, right 880 Hz.
fn stereo_tone(frame_index: usize) -> [f32; STEREO_FRAME_SAMPLES] {
    let mut frame = [0.0f32; STEREO_FRAME_SAMPLES];
    for i in 0..STEREO_FRAME_SAMPLES / 2 {
        let t = (frame_index * STEREO_FRAME_SAMPLES / 2 + i) as f32 / 48_000.0;
        frame[i * 2] = 0.4 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        frame[i * 2 + 1] = 0.4 * (2.0 * std::f32::consts::PI * 880.0 * t).sin();
    }
    frame
}

fn screen_audio_packets(channel: u32, peer: u32, count: usize) -> Vec<MediaPacket> {
    let mut encoder = ScreenAudioEncoder::new(96).expect("encoder");
    (0..count)
        .map(|i| {
            encoder
                .packetize(channel, peer, i as u16, (i * 960) as u32, false, &stereo_tone(i))
                .expect("packet")
        })
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

#[test]
fn mixer_plays_a_stereo_screen_stream_without_speech_events() {
    let mut mixer = AudioMixer::new();
    let mut now = Instant::now();
    let packets = screen_audio_packets(1, 7, 80);

    let (mut speech_events, mut loud_frames) = (0, 0);
    for packet in &packets {
        // Packets arrive and are played at the 20 ms cadence of a live stream.
        if mixer.on_screen_audio_packet(&packet.header, packet.payload.clone(), now).is_some() {
            speech_events += 1;
        }
        let mut out = [0.0f32; FRAME_SIZE_48K];
        let events = mixer.mix_frame(&mut out, now);
        assert!(events.iter().all(|(peer, _)| *peer != screen_audio_key(7)), "no events for the screen channel");
        if rms(&out) > 0.1 {
            loud_frames += 1;
        }
        now += Duration::from_millis(20);
    }
    assert_eq!(speech_events, 0, "screen sound must not light a speaking indicator");
    assert!(loud_frames >= 60, "only {loud_frames} audible frames out of 80");
}

#[test]
fn screen_sound_has_its_own_volume() {
    let mut mixer = AudioMixer::new();
    let mut now = Instant::now();
    let packets = screen_audio_packets(1, 7, 60);
    // The volume slider exists once the stream is there.
    mixer.on_screen_audio_packet(&packets[0].header, packets[0].payload.clone(), now);
    mixer.set_speaker_volume(screen_audio_key(7), 0.0);

    let mut out = [0.0f32; FRAME_SIZE_48K];
    for packet in &packets[1..] {
        mixer.on_screen_audio_packet(&packet.header, packet.payload.clone(), now);
        mixer.mix_frame(&mut out, now);
        assert!(rms(&out) < 1e-4, "volume 0 must silence the screen channel");
        now += Duration::from_millis(20);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn viewers_receive_the_screen_sound_even_when_the_sharer_is_muted() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    a.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 1280, height: 720, fps: 30 }).await?;
    let a_peer = a.session.session_peer_id;
    b.expect_control(WAIT, |m| match m {
        ServerMessage::ScreenShareStarted { peer_id, .. } if *peer_id == a_peer => Some(()),
        _ => None,
    })
    .await?;
    b.send(ClientMessage::SubscribeScreenShare { target_user_id: a.session.user_id }).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Muting the microphone must not silence the game sound.
    a.send(ClientMessage::UpdateVoiceState { muted: true, deafened: false }).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (tx, rx) = mpsc::channel();
    let mut forwarder = ScreenAudioForwarder::start(a.session.connection.clone(), move || Ok(Box::new(rx)))
        .map_err(|e| anyhow::anyhow!(e))?;
    for packet in screen_audio_packets(voice_short, a_peer, 10) {
        tx.send(packet)?;
    }

    let mut received = 0;
    while received < 5 {
        let datagram = b.wait_datagram(WAIT).await?;
        let header = MediaHeader::from_slice(&datagram[..MEDIA_HEADER_SIZE])?;
        if header.media_type == MediaType::ScreenAudio {
            assert_eq!(header.sender_peer_id, a_peer);
            received += 1;
        }
    }
    forwarder.stop();
    Ok(())
}

/// Screen media that reached one viewer during a wait.
#[derive(Debug, Default)]
struct ScreenMediaSeen {
    screen_audio: usize,
    video: usize,
    keyframes: usize,
}

/// Send one burst of screen media from `a`: screen sound, a P-frame, and an IDR on a uni-stream.
async fn send_screen_media(a: &TestClient, channel: u32, peer: u32, frames: usize) -> anyhow::Result<()> {
    for packet in screen_audio_packets(channel, peer, frames) {
        a.session.send_datagram(packet.encode())?;
    }
    let delta: Vec<u8> = (0..3000).map(|i| (i % 199) as u8).collect();
    for packet in slice_p_frame(channel, peer, 1, 2, 1033, &delta, false) {
        a.session.send_datagram(packet.encode())?;
    }
    let header = MediaHeader::new_video_key(channel, peer, 0, 1000, 1, false);
    let mut uni = a.session.connection.open_uni().await?;
    uni.write_all(&header.to_bytes()).await?;
    uni.write_all(&[7u8; 4096]).await?;
    uni.finish()?;
    Ok(())
}

/// Count the screen media that reaches `viewer` for `wait`.
async fn screen_media_seen(viewer: &mut TestClient, wait: Duration) -> ScreenMediaSeen {
    let mut seen = ScreenMediaSeen::default();
    viewer
        .pump_events(tokio::time::Instant::now() + wait, |event| match event {
            SessionEvent::Datagram(datagram) => match MediaHeader::from_slice(datagram) {
                Ok(header) if header.media_type == MediaType::ScreenAudio => seen.screen_audio += 1,
                Ok(header) if header.media_type == MediaType::VideoDelta => seen.video += 1,
                _ => {}
            },
            SessionEvent::UniStream { .. } => seen.keyframes += 1,
            _ => {}
        })
        .await;
    seen
}

#[tokio::test(flavor = "multi_thread")]
async fn screen_media_without_a_share_is_not_forwarded() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    let a_peer = a.session.session_peer_id;
    // Bob watches Alice, who has not started a share.
    b.send(ClientMessage::SubscribeScreenShare { target_user_id: a.session.user_id }).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    send_screen_media(&a, voice_short, a_peer, 10).await?;
    let before = screen_media_seen(&mut b, Duration::from_millis(500)).await;
    assert_eq!(
        (before.screen_audio, before.video, before.keyframes),
        (0, 0, 0),
        "screen media without a started share reached a viewer"
    );

    // Once the share starts, the same media is forwarded.
    a.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 1280, height: 720, fps: 30 }).await?;
    b.expect_control(WAIT, |m| matches!(m, ServerMessage::ScreenShareStarted { peer_id, .. } if *peer_id == a_peer).then_some(())).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    send_screen_media(&a, voice_short, a_peer, 10).await?;
    let after = screen_media_seen(&mut b, Duration::from_millis(500)).await;
    assert!(after.screen_audio > 0, "screen sound of a live share was not forwarded");
    assert!(after.video > 0, "screen video of a live share was not forwarded");
    assert!(after.keyframes > 0, "keyframes of a live share were not forwarded");
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_muted_sharer_keeps_video_but_not_screen_sound() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let moderator = TestClient::register(&server, "moddy").await?;
    boss.api.set_user_role(moderator.session.user_id, "moderator").await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    let a_peer = a.session.session_peer_id;

    a.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 1280, height: 720, fps: 30 }).await?;
    b.expect_control(WAIT, |m| matches!(m, ServerMessage::ScreenShareStarted { peer_id, .. } if *peer_id == a_peer).then_some(())).await?;
    b.send(ClientMessage::SubscribeScreenShare { target_user_id: a.session.user_id }).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // A moderator silences Alice while her share goes on.
    moderator.api.moderate_voice(a.session.user_id, "mute").await?;
    b.expect_control(WAIT, |m| matches!(m, ServerMessage::ServerMuteChanged { peer_id, server_muted: true } if *peer_id == a_peer).then_some(())).await?;

    send_screen_media(&a, voice_short, a_peer, 10).await?;
    let seen = screen_media_seen(&mut b, Duration::from_millis(500)).await;
    assert_eq!(seen.screen_audio, 0, "screen sound of a server-muted sharer reached a viewer");
    assert!(seen.video > 0, "screen video of a server-muted sharer must still reach viewers");
    Ok(())
}
