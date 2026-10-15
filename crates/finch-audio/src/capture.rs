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
        })
    }

    /// Forget the echo path (reference stream interrupted, device changed).
    pub fn reset_echo_canceller(&mut self) {
        self.echo_canceller.reset();
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
        self.echo_canceller.process_capture(frame);
        self.noise_suppressor.process_frame(frame);

        let vad = self.vad.process(frame);
        if !vad.should_transmit() {
            self.transmitting = false;
            if self.preroll.len() == PREROLL_FRAMES {
                self.preroll.pop_front();
            }
            self.preroll.push_back(*frame);
            return CaptureOutput { packets: Vec::new(), vad };
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

        CaptureOutput { packets, vad }
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
}
