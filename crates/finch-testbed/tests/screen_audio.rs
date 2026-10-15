//! Sound of a shared screen (game, video): sent as `ScreenAudio` datagrams to the viewers and
//! played through its own mixer channel.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use finch_audio::{screen_audio_key, AudioMixer, FRAME_SIZE_48K};
use finch_client_core::screen::ScreenAudioForwarder;
use finch_protocol::media::{MediaHeader, MediaPacket, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::VideoCodec;
use finch_screen::{ScreenAudioEncoder, STEREO_FRAME_SAMPLES};
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
