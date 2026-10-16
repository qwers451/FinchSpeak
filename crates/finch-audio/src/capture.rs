//! Voice capture pipeline: AEC → noise suppression → VAD → Opus → media packet.
//!
//! Pure processing without audio I/O, so the same code runs inside the cpal
//! worker thread of the client and inside headless test harnesses.

use std::collections::VecDeque;

use bytes::{BufMut, Bytes, BytesMut};
use finch_protocol::media::{MediaHeader, MEDIA_HEADER_SIZE};

use crate::aec::EchoCanceller;
use crate::noise::NoiseSuppressor;
use crate::opus::{AudioOpusError, OpusVoiceEncoder};
use crate::resampler::FRAME_SIZE_48K;
use crate::vad::{VadDecision, VadMode, VoiceActivityDetector, DEFAULT_VAD_THRESHOLD_DB};

/// Frames of audio kept from just before the VAD opens and sent first when it does.
/// The speech classifier reacts 1-3 frames (20-60 ms) after a word starts (measured on three
/// voices); one extra frame is kept as margin. Without this the start of every word is clipped.
pub const PREROLL_FRAMES: usize = 4;

/// Largest microphone boost (+18 dB).
pub const MAX_INPUT_GAIN: f32 = 8.0;

/// Largest output-device delay passed on to the echo canceller.
pub const MAX_STREAM_DELAY_MS: u32 = 500;

/// Duration of one frame in the packet timestamps.
const FRAME_MS: u32 = 20;

/// Static parameters of a capture pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceCaptureConfig {
    /// Short channel ID written into every media header.
    pub channel_short_id: u32,
    /// Opus bitrate in kbit/s.
    pub bitrate_kbps: u32,
    /// VAD RMS threshold in dBFS.
    pub vad_threshold_db: f32,
    /// How speech is detected. The default is the speech classifier.
    pub vad_mode: VadMode,
}

impl Default for VoiceCaptureConfig {
    fn default() -> Self {
        Self {
            channel_short_id: 0,
            bitrate_kbps: 64,
            vad_threshold_db: DEFAULT_VAD_THRESHOLD_DB,
            vad_mode: VadMode::default(),
        }
    }
}

/// Result of processing one 20 ms capture frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureOutput {
    /// Media datagrams (20-byte header + Opus payload) to send, oldest first. Usually one; when
    /// the VAD opens, the pre-roll frames come first. Empty while silent.
    pub packets: Vec<Bytes>,
    /// VAD decision for this frame.
    pub vad: VadDecision,
    /// Level of the processed frame in dBFS (-120 for silence): what the threshold is compared with.
    pub level_db: f32,
}

/// Microphone-side voice pipeline for a single voice channel session.
pub struct VoiceCapture {
    config: VoiceCaptureConfig,
    echo_canceller: EchoCanceller,
    noise_suppressor: NoiseSuppressor,
    vad: VoiceActivityDetector,
    encoder: OpusVoiceEncoder,
    seq: u16,
    /// Last silent frames, oldest first (at most [`PREROLL_FRAMES`]).
    preroll: VecDeque<[f32; FRAME_SIZE_48K]>,
    /// True from the moment the VAD opens until it is back to `Silence`.
    transmitting: bool,
    /// Microphone volume multiplier (1.0 = unchanged).
    input_gain: f32,
    /// Push-to-talk: while false nothing is transmitted.
    gate_open: bool,
}

impl VoiceCapture {
    pub fn new(config: VoiceCaptureConfig) -> Result<Self, AudioOpusError> {
        Ok(Self {
            config,
            echo_canceller: EchoCanceller::new(),
            noise_suppressor: NoiseSuppressor::new(),
            vad: VoiceActivityDetector::with_mode(config.vad_mode, config.vad_threshold_db),
            encoder: OpusVoiceEncoder::new(config.bitrate_kbps)?,
            seq: 0,
            preroll: VecDeque::with_capacity(PREROLL_FRAMES),
            transmitting: false,
            input_gain: 1.0,
            gate_open: true,
        })
    }

    /// Forget the echo path (reference stream interrupted, device changed).
    pub fn reset_echo_canceller(&mut self) {
        self.echo_canceller.reset();
    }

    /// Microphone volume multiplier, limited to 0..=[`MAX_INPUT_GAIN`]. Applied before echo
    /// cancellation and the speech detector, so a quiet microphone can be brought above the VAD threshold.
    pub fn set_input_gain(&mut self, gain: f32) {
        self.input_gain = if gain.is_finite() { gain.clamp(0.0, MAX_INPUT_GAIN) } else { 1.0 };
    }

    /// Push-to-talk gate: `false` stops all transmission (the pipeline keeps adapting).
    pub fn set_transmit_gate(&mut self, open: bool) {
        self.gate_open = open;
    }

    /// Change the loudness threshold of the speech detector (dBFS).
    pub fn set_vad_threshold_db(&mut self, threshold_db: f32) {
        self.vad.set_threshold(threshold_db);
    }

    /// Tell the echo canceller how long played audio takes to reach the speakers (the output
    /// device's buffer). Clamped to [`MAX_STREAM_DELAY_MS`]; the canceller still tracks the rest.
    pub fn set_stream_delay_ms(&mut self, delay_ms: u32) {
        let delay = delay_ms.min(MAX_STREAM_DELAY_MS) as i32;
        if let Err(error) = self.echo_canceller.set_stream_delay_ms(delay) {
            tracing::warn!(%error, delay, "echo canceller rejected the stream delay");
        }
    }

    /// Feed the 48 kHz frame that is being sent to the speakers (AEC far-end reference).
    pub fn push_playback_reference(&mut self, frame: &[f32; FRAME_SIZE_48K]) {
        self.echo_canceller.push_reference(frame);
    }

    /// Process one 48 kHz mono capture frame in place and build the outgoing packet.
    ///
    /// After the call `frame` holds the processed signal (before Opus encoding).
    pub fn process(
        &mut self,
        frame: &mut [f32; FRAME_SIZE_48K],
        sender_peer_id: u32,
        timestamp_ms: u32,
    ) -> CaptureOutput {
        if (self.input_gain - 1.0).abs() > f32::EPSILON {
            for sample in frame.iter_mut() {
                *sample = (*sample * self.input_gain).clamp(-1.0, 1.0);
            }
        }
        self.echo_canceller.process_capture(frame);
        self.noise_suppressor.process_frame(frame);

        let level_db = VoiceActivityDetector::compute_rms_dbfs(frame);
        let mut vad = self.vad.process(frame);
        if !self.gate_open {
            vad = VadDecision::Silence;
        }
        if !vad.should_transmit() {
            self.transmitting = false;
            if self.preroll.len() == PREROLL_FRAMES {
                self.preroll.pop_front();
            }
            self.preroll.push_back(*frame);
            return CaptureOutput { packets: Vec::new(), vad, level_db };
        }

        let mut packets = Vec::with_capacity(PREROLL_FRAMES + 1);
        if !self.transmitting {
            let frames: Vec<_> = self.preroll.drain(..).collect();
            let count = frames.len() as u32;
            for (index, old) in frames.iter().enumerate() {
                let age = count - index as u32;
                let timestamp = timestamp_ms.wrapping_sub(age * FRAME_MS);
                packets.extend(self.encode_packet(old, sender_peer_id, timestamp, false));
            }
        }
        self.transmitting = true;
        packets.extend(self.encode_packet(frame, sender_peer_id, timestamp_ms, vad.is_silence_packet()));

        CaptureOutput { packets, vad, level_db }
    }

    /// Opus-encode one frame into a complete media datagram; `None` if encoding fails.
    fn encode_packet(
        &mut self,
        frame: &[f32; FRAME_SIZE_48K],
        sender_peer_id: u32,
        timestamp_ms: u32,
        silence_flag: bool,
    ) -> Option<Bytes> {
        let opus_bytes = match self.encoder.encode(frame) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Opus encoding failed: {}", e);
                return None;
            }
        };

        let current_seq = self.seq;
        self.seq = self.seq.wrapping_add(1);

        let header = MediaHeader::new_audio(
            self.config.channel_short_id,
            sender_peer_id,
            current_seq,
            timestamp_ms,
            silence_flag,
            false,
        );

        let mut packet = BytesMut::with_capacity(MEDIA_HEADER_SIZE + opus_bytes.len());
        header.write_to(&mut packet);
        packet.put_slice(&opus_bytes);
        Some(packet.freeze())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opus::OpusVoiceDecoder;
    use crate::resampler::FRAME_SIZE_48K;
    use crate::vad::{VadDecision, VadMode};
    use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};

    fn tone_frame(frame_index: usize) -> [f32; FRAME_SIZE_48K] {
        let mut frame = [0.0f32; FRAME_SIZE_48K];
        for (i, s) in frame.iter_mut().enumerate() {
            let t = (frame_index * FRAME_SIZE_48K + i) as f32 / 48_000.0;
            *s = 0.3 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        }
        frame
    }

    /// Loudness-threshold VAD: these tests exercise packet layout and hangover on a synthetic tone.
    fn capture_for_test() -> VoiceCapture {
        VoiceCapture::new(VoiceCaptureConfig {
            channel_short_id: 7,
            vad_mode: VadMode::Energy,
            ..VoiceCaptureConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn output_reports_the_level_of_the_processed_frame() {
        let mut capture = capture_for_test();
        let mut silence = [0.0f32; FRAME_SIZE_48K];
        let quiet = capture.process(&mut silence, 1, 0).level_db;
        assert!(quiet <= -100.0, "silence is reported near the floor, got {quiet}");

        let mut loud = tone_frame(0);
        let mut level = f32::MIN;
        for i in 0..5 {
            let mut frame = tone_frame(i);
            level = capture.process(&mut frame, 1, (i as u32 + 1) * 20).level_db;
            loud = frame;
        }
        assert!(level > -30.0 && level <= 0.0, "a tone shows up as a level, got {level}");
        assert!((level - VoiceActivityDetector::compute_rms_dbfs(&loud)).abs() < 1.0, "it is the level the threshold sees");
    }

    #[test]
    fn stream_delay_hint_is_accepted_and_clamped() {
        let mut capture = capture_for_test();
        for delay in [0, 40, 500, 10_000, u32::MAX] {
            capture.set_stream_delay_ms(delay); // must not panic or poison the canceller
        }
        let mut frame = tone_frame(0);
        capture.process(&mut frame, 1, 0);
        assert!(frame.iter().all(|sample| sample.is_finite()), "the canceller still produces sane audio");
    }

    #[test]
    fn voice_capture_matches_legacy_packet_layout() {
        let mut capture = capture_for_test();
        let mut decoder = OpusVoiceDecoder::new().unwrap();

        for i in 0..5 {
            let mut frame = tone_frame(i);
            let timestamp_ms = 1_000 + i as u32 * 20;
            let out = capture.process(&mut frame, 42, timestamp_ms);
            let packet = out.packets.last().expect("tone frame must be transmitted").clone();

            assert!(packet.len() > MEDIA_HEADER_SIZE);
            let header = MediaHeader::from_slice(&packet[..MEDIA_HEADER_SIZE]).unwrap();
            assert_eq!(header.media_type, MediaType::Audio);
            assert_eq!(header.channel_id, 7);
            assert_eq!(header.sender_peer_id, 42);
            assert_eq!(header.sequence, i as u16);
            assert_eq!(header.timestamp, timestamp_ms);
            assert!(!header.is_silence());
            assert!(!header.is_e2ee());

            let mut pcm = [0.0f32; FRAME_SIZE_48K];
            decoder.decode(&packet[MEDIA_HEADER_SIZE..], &mut pcm).unwrap();
        }
    }

    #[test]
    fn voice_capture_silence_not_transmitted() {
        let mut capture = capture_for_test();
        for i in 0..5 {
            let mut frame = tone_frame(i);
            capture.process(&mut frame, 42, 0);
        }

        let mut outputs = Vec::new();
        for _ in 0..10 {
            let mut frame = [0.0f32; FRAME_SIZE_48K];
            outputs.push(capture.process(&mut frame, 42, 0));
        }

        // Hangover frames are still sent and the last one carries the silence flag
        // (RNNoise latency may keep the first zero frame above the VAD threshold).
        let last_hangover = outputs
            .iter()
            .position(|o| o.vad == VadDecision::Hangover { is_last: true })
            .expect("hangover must end within 10 frames");
        let packet = outputs[last_hangover].packets.last().expect("last hangover frame is sent");
        assert!(MediaHeader::from_slice(&packet[..MEDIA_HEADER_SIZE]).unwrap().is_silence());
        assert!(outputs.len() - last_hangover > 3, "silence must follow the hangover");
        for out in &outputs[last_hangover + 1..] {
            assert!(out.packets.is_empty());
            assert_eq!(out.vad, VadDecision::Silence);
        }
    }

    #[test]
    fn voice_capture_sequence_increments_only_on_send() {
        let mut capture = capture_for_test();
        let mut last_seq_before_pause = None;
        for i in 0..5 {
            let mut frame = tone_frame(i);
            capture.process(&mut frame, 42, 0);
        }
        for _ in 0..10 {
            let mut frame = [0.0f32; FRAME_SIZE_48K];
            for p in capture.process(&mut frame, 42, 0).packets {
                last_seq_before_pause = Some(MediaHeader::from_slice(&p[..MEDIA_HEADER_SIZE]).unwrap().sequence);
            }
        }

        let mut frame = tone_frame(100);
        let packet = capture.process(&mut frame, 42, 0).packets.into_iter().next().expect("speech resumes");
        let seq_after = MediaHeader::from_slice(&packet[..MEDIA_HEADER_SIZE]).unwrap().sequence;
        assert_eq!(Some(seq_after.wrapping_sub(1)), last_seq_before_pause);
    }

    /// Pink noise (Paul Kellet's filter) at `rms_dbfs`, deterministic.
    fn pink_noise(len: usize, rms_dbfs: f32) -> Vec<f32> {
        let mut state = 7u32;
        let mut b = [0.0f32; 7];
        let raw: Vec<f32> = (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (state >> 8) as f32 / 8_388_608.0 - 1.0;
                b[0] = 0.99886 * b[0] + white * 0.055_517_9;
                b[1] = 0.99332 * b[1] + white * 0.075_075_9;
                b[2] = 0.96900 * b[2] + white * 0.153_852;
                b[3] = 0.86650 * b[3] + white * 0.310_485_6;
                b[4] = 0.55000 * b[4] + white * 0.532_952_2;
                b[5] = -0.7616 * b[5] - white * 0.016_898;
                let out = b.iter().sum::<f32>() + white * 0.5362;
                b[6] = white * 0.115_926;
                out
            })
            .collect();
        let power = raw.iter().map(|s| s * s).sum::<f32>() / raw.len() as f32;
        let gain = (10f32.powf(rms_dbfs / 10.0) / power).sqrt();
        raw.iter().map(|s| s * gain).collect()
    }

    /// A fan at -35 dBFS is loud but is not speech (AUD-050): nothing may be transmitted,
    /// and the indicator must stay dark. (Positive control with real speech: finch-testbed.)
    #[test]
    fn speech_mode_does_not_transmit_loud_noise() {
        let mut capture = VoiceCapture::new(VoiceCaptureConfig::default()).unwrap();
        assert_eq!(VoiceCaptureConfig::default().vad_mode, VadMode::Speech);

        let noise = pink_noise(3 * 48_000, -35.0);
        let mut packets = 0;
        let mut speaking = 0;
        let (chunks, _) = noise.as_chunks::<FRAME_SIZE_48K>();
        for (i, chunk) in chunks.iter().enumerate() {
            let mut frame = *chunk;
            let out = capture.process(&mut frame, 42, i as u32 * 20);
            packets += out.packets.len();
            speaking += usize::from(out.vad == VadDecision::Speaking);
        }
        assert_eq!(speaking, 0, "noise was classified as speech");
        assert_eq!(packets, 0, "noise was transmitted");
    }

    /// When the gate opens, the frames just before it are sent first, so the start of a word
    /// is not clipped by the detector's reaction time.
    #[test]
    fn opening_the_gate_sends_a_preroll() {
        let mut capture = capture_for_test();
        let silent_frames = PREROLL_FRAMES as u32 + 3;
        for i in 0..silent_frames {
            let mut silence = [0.0f32; FRAME_SIZE_48K];
            assert!(capture.process(&mut silence, 42, i * 20).packets.is_empty());
        }

        let mut frame = tone_frame(0);
        let out = capture.process(&mut frame, 42, silent_frames * 20);
        assert_eq!(out.packets.len(), PREROLL_FRAMES + 1, "pre-roll + the frame itself");

        let headers: Vec<MediaHeader> = out
            .packets
            .iter()
            .map(|p| MediaHeader::from_slice(&p[..MEDIA_HEADER_SIZE]).unwrap())
            .collect();
        let expected_ts: Vec<u32> = (silent_frames - PREROLL_FRAMES as u32..=silent_frames).map(|k| k * 20).collect();
        assert_eq!(headers.iter().map(|h| h.timestamp).collect::<Vec<_>>(), expected_ts);
        assert_eq!(headers.iter().map(|h| h.sequence).collect::<Vec<_>>(), (0..=PREROLL_FRAMES as u16).collect::<Vec<_>>());
        assert!(headers.iter().all(|h| !h.is_silence()));

        // Staying open does not repeat the pre-roll.
        let mut next = tone_frame(1);
        assert_eq!(capture.process(&mut next, 42, (silent_frames + 1) * 20).packets.len(), 1);
    }

    // ---- microphone gain, push-to-talk gate, VAD threshold

    fn rms_of(frame: &[f32; FRAME_SIZE_48K]) -> f32 {
        (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
    }

    /// A source at about -23 dBFS: well below a -15 dBFS threshold, loud enough for the noise
    /// suppressor to leave it alone.
    fn quiet_frame(frame_index: usize) -> [f32; FRAME_SIZE_48K] {
        let mut frame = tone_frame(frame_index);
        frame.iter_mut().for_each(|s| *s /= 3.0);
        frame
    }

    /// Mean level of the processed signal after a few frames of warm-up.
    fn processed_level(gain: f32) -> f32 {
        let mut capture = capture_for_test();
        capture.set_input_gain(gain);
        let mut total = 0.0;
        for i in 0..20 {
            let mut frame = quiet_frame(i);
            capture.process(&mut frame, 1, i as u32 * 20);
            if i >= 10 {
                total += rms_of(&frame);
            }
        }
        total / 10.0
    }

    #[test]
    fn input_gain_scales_the_signal_and_is_limited() {
        let (normal, doubled) = (processed_level(1.0), processed_level(2.0));
        assert!(doubled > normal * 1.4, "gain 2.0: {doubled} vs {normal}");
        let silent = processed_level(0.0);
        assert!(silent < normal * 0.05, "gain 0 mutes: {silent} vs {normal}");

        let mut capture = capture_for_test();
        capture.set_input_gain(100.0); // absurd values are limited
        let mut loud = tone_frame(1);
        capture.process(&mut loud, 1, 20);
        assert!(loud.iter().all(|s| s.abs() <= 1.0), "boosted audio is clipped, never wraps");
        capture.set_input_gain(f32::NAN);
        let mut frame = quiet_frame(2);
        capture.process(&mut frame, 1, 40);
        assert!(frame.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn a_boosted_quiet_microphone_starts_to_transmit() {
        let mut capture = VoiceCapture::new(VoiceCaptureConfig {
            vad_mode: VadMode::Energy,
            vad_threshold_db: -15.0,
            ..VoiceCaptureConfig::default()
        })
        .unwrap();
        let mut silent_packets = 0;
        for i in 0..10 {
            let mut frame = quiet_frame(i);
            silent_packets += capture.process(&mut frame, 1, i as u32 * 20).packets.len();
        }
        assert_eq!(silent_packets, 0, "a -23 dBFS source is below the -15 dBFS threshold");

        capture.set_input_gain(4.0); // +12 dB
        let mut packets = 0;
        for i in 10..20 {
            let mut frame = quiet_frame(i);
            packets += capture.process(&mut frame, 1, i as u32 * 20).packets.len();
        }
        assert!(packets > 0, "after the boost the same source is heard");
    }

    #[test]
    fn a_closed_gate_sends_nothing_and_an_open_one_does() {
        let mut capture = capture_for_test();
        capture.set_transmit_gate(false);
        for i in 0..10 {
            let mut frame = tone_frame(i);
            let out = capture.process(&mut frame, 1, i as u32 * 20);
            assert!(out.packets.is_empty(), "push-to-talk released: nothing leaves the machine");
            assert_eq!(out.vad, VadDecision::Silence, "and the speaking indicator stays dark");
        }
        capture.set_transmit_gate(true);
        let mut frame = tone_frame(10);
        let out = capture.process(&mut frame, 1, 200);
        assert!(!out.packets.is_empty());
    }

    #[test]
    fn the_vad_threshold_can_be_changed_while_running() {
        let mut capture = VoiceCapture::new(VoiceCaptureConfig {
            vad_mode: VadMode::Energy,
            vad_threshold_db: -15.0,
            ..VoiceCaptureConfig::default()
        })
        .unwrap();
        let mut frame = quiet_frame(0);
        assert!(capture.process(&mut frame, 1, 0).packets.is_empty());
        capture.set_vad_threshold_db(-60.0);
        let mut frame = quiet_frame(1);
        assert!(!capture.process(&mut frame, 1, 20).packets.is_empty(), "now it is loud enough");
    }
}
