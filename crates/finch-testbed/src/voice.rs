//! Real-time voice link: client A's capture pipeline → real SFU → client B's mixer.

use std::time::{Duration, Instant};

use finch_audio::{AudioMixer, VadMode, VoiceCapture, VoiceCaptureConfig, FRAME_SIZE_48K};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::models::ChannelType;

use crate::client::TestClient;

/// Everything observed on the receiving side of a voice link run.
#[derive(Debug, Default)]
pub struct VoiceRun {
    /// Mixer output of the receiver, 48 kHz mono, one frame per 20 ms tick.
    pub output: Vec<f32>,
    /// `(frame index, peer id, speaking)` transitions reported by the receiver's mixer.
    pub speaking_events: Vec<(usize, u32, bool)>,
    /// Audio datagrams the sender transmitted.
    pub sent_packets: usize,
    /// Headers of all audio datagrams the receiver got.
    pub received_headers: Vec<MediaHeader>,
    /// Largest jitter buffer depth seen during the run.
    pub max_buffered_packets: usize,
    /// Receiver-side loss rate for the sender at the end of the run.
    pub loss_rate: Option<f64>,
    /// Scheduling delay in the test harness (not a product latency measurement).
    pub max_tick_lateness: Duration,
    /// Longest synchronous DSP call in the test harness.
    pub max_capture_time: Duration,
}

/// Stream `input` (48 kHz mono) from `sender` to `receiver` in real time, 20 ms per frame,
/// followed by `tail_frames` frames of silence. Both clients must already be in the voice channel.
pub async fn run_voice_link(
    sender: &TestClient,
    receiver: &mut TestClient,
    input: &[f32],
    tail_frames: usize,
) -> anyhow::Result<VoiceRun> {
    let defaults = VoiceCaptureConfig::default();
    run_voice_link_with(sender, receiver, input, tail_frames, defaults.vad_mode, defaults.vad_threshold_db).await
}

/// Like [`run_voice_link`] with an explicit VAD. Synthetic probes (chirps, tones) are not speech:
/// use [`VadMode::Energy`], and a very low threshold (e.g. −100 dB) to transmit every frame and
/// isolate network effects from VAD pauses.
pub async fn run_voice_link_with(
    sender: &TestClient,
    receiver: &mut TestClient,
    input: &[f32],
    tail_frames: usize,
    vad_mode: VadMode,
    vad_threshold_db: f32,
) -> anyhow::Result<VoiceRun> {
    run_voice_link_with_config(sender, receiver, input, tail_frames, VoiceCaptureConfig { vad_mode, vad_threshold_db, ..Default::default() }).await
}

/// Full production capture configuration; old helpers retain default-on enhancement.
pub async fn run_voice_link_with_config(
    sender: &TestClient, receiver: &mut TestClient, input: &[f32], tail_frames: usize,
    mut config: VoiceCaptureConfig,
) -> anyhow::Result<VoiceRun> {
    let (_, short_id) = sender.channel(ChannelType::Voice);
    let sender_peer = sender.session.session_peer_id;
    config.channel_short_id = short_id;
    let mut capture = VoiceCapture::new(config)?;
    let mut mixer = AudioMixer::new();
    let mut run = VoiceRun::default();

    let total_frames = input.len().div_ceil(FRAME_SIZE_48K) + tail_frames;
    let mut ticker = tokio::time::interval(Duration::from_millis(20));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);

    for frame_index in 0..total_frames {
        let scheduled = ticker.tick().await;
        run.max_tick_lateness = run.max_tick_lateness.max(tokio::time::Instant::now().saturating_duration_since(scheduled));

        let mut frame = [0.0f32; FRAME_SIZE_48K];
        let start = frame_index * FRAME_SIZE_48K;
        if start < input.len() {
            let end = (start + FRAME_SIZE_48K).min(input.len());
            frame[..end - start].copy_from_slice(&input[start..end]);
        }
        let capture_started = Instant::now();
        let out = capture.process(&mut frame, sender_peer, (frame_index * 20) as u32)?;
        run.max_capture_time = run.max_capture_time.max(capture_started.elapsed());
        for packet in out.packets {
            sender.session.send_datagram(packet)?;
            run.sent_packets += 1;
        }

        let now = Instant::now();
        for datagram in receiver.drain_datagrams() {
            let Ok(header) = MediaHeader::from_slice(&datagram[..MEDIA_HEADER_SIZE.min(datagram.len())]) else {
                continue;
            };
            if header.media_type != MediaType::Audio {
                continue;
            }
            run.received_headers.push(header);
            if let Some((peer, speaking)) =
                mixer.on_audio_packet(&header, datagram.slice(MEDIA_HEADER_SIZE..), now)
            {
                run.speaking_events.push((frame_index, peer, speaking));
            }
        }
        run.max_buffered_packets = run.max_buffered_packets.max(mixer.max_buffered_packets());

        let mut mixed = [0.0f32; FRAME_SIZE_48K];
        for (peer, speaking) in mixer.mix_frame(&mut mixed, now) {
            run.speaking_events.push((frame_index, peer, speaking));
        }
        run.output.extend_from_slice(&mixed);
    }

    run.loss_rate = mixer.loss_rate(sender_peer);
    Ok(run)
}
