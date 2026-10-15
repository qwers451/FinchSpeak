//! Voice capture pipeline: AEC → noise suppression → VAD → Opus → media packet.
//!
//! Pure processing without audio I/O, so the same code runs inside the cpal
//! worker thread of the client and inside headless test harnesses.

use bytes::{BufMut, Bytes, BytesMut};
use finch_protocol::media::{MediaHeader, MEDIA_HEADER_SIZE};

use crate::aec::EchoCanceller;
use crate::noise::NoiseSuppressor;
use crate::opus::{AudioOpusError, OpusVoiceEncoder};
use crate::resampler::FRAME_SIZE_48K;
use crate::vad::{VadDecision, VoiceActivityDetector, DEFAULT_VAD_THRESHOLD_DB};

/// Static parameters of a capture pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceCaptureConfig {
    /// Short channel ID written into every media header.
    pub channel_short_id: u32,
    /// Opus bitrate in kbit/s.
    pub bitrate_kbps: u32,
    /// VAD RMS threshold in dBFS.
    pub vad_threshold_db: f32,
}

impl Default for VoiceCaptureConfig {
    fn default() -> Self {
        Self {
            channel_short_id: 0,
            bitrate_kbps: 64,
            vad_threshold_db: DEFAULT_VAD_THRESHOLD_DB,
        }
    }
}

/// Result of processing one 20 ms capture frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureOutput {
    /// Complete media datagram (20-byte header + Opus payload), if the frame is transmitted.
    pub packet: Option<Bytes>,
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
}

impl VoiceCapture {
    pub fn new(config: VoiceCaptureConfig) -> Result<Self, AudioOpusError> {
        Ok(Self {
            config,
            echo_canceller: EchoCanceller::new(),
            noise_suppressor: NoiseSuppressor::new(),
            vad: VoiceActivityDetector::new(config.vad_threshold_db),
            encoder: OpusVoiceEncoder::new(config.bitrate_kbps)?,
            seq: 0,
        })
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
            return CaptureOutput { packet: None, vad };
        }

        let opus_bytes = match self.encoder.encode(frame) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Opus encoding failed: {}", e);
                return CaptureOutput { packet: None, vad };
            }
        };

        let current_seq = self.seq;
        self.seq = self.seq.wrapping_add(1);

        let header = MediaHeader::new_audio(
            self.config.channel_short_id,
            sender_peer_id,
            current_seq,
            timestamp_ms,
            vad.is_silence_packet(),
            false,
        );

        let mut packet = BytesMut::with_capacity(MEDIA_HEADER_SIZE + opus_bytes.len());
        header.write_to(&mut packet);
        packet.put_slice(&opus_bytes);

        CaptureOutput {
            packet: Some(packet.freeze()),
            vad,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opus::OpusVoiceDecoder;
    use crate::resampler::FRAME_SIZE_48K;
    use crate::vad::VadDecision;
    use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};

    fn tone_frame(frame_index: usize) -> [f32; FRAME_SIZE_48K] {
        let mut frame = [0.0f32; FRAME_SIZE_48K];
        for (i, s) in frame.iter_mut().enumerate() {
            let t = (frame_index * FRAME_SIZE_48K + i) as f32 / 48_000.0;
            *s = 0.3 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        }
        frame
    }

    fn capture_for_test() -> VoiceCapture {
        VoiceCapture::new(VoiceCaptureConfig {
            channel_short_id: 7,
            ..VoiceCaptureConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn voice_capture_matches_legacy_packet_layout() {
        let mut capture = capture_for_test();
        let mut decoder = OpusVoiceDecoder::new().unwrap();

        for i in 0..5 {
            let mut frame = tone_frame(i);
            let timestamp_ms = 1_000 + i as u32 * 20;
            let out = capture.process(&mut frame, 42, timestamp_ms);
            let packet = out.packet.expect("tone frame must be transmitted");

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
        let packet = outputs[last_hangover].packet.as_ref().expect("last hangover frame is sent");
        assert!(MediaHeader::from_slice(&packet[..MEDIA_HEADER_SIZE]).unwrap().is_silence());
        assert!(outputs.len() - last_hangover > 3, "silence must follow the hangover");
        for out in &outputs[last_hangover + 1..] {
            assert!(out.packet.is_none());
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
            if let Some(p) = capture.process(&mut frame, 42, 0).packet {
                last_seq_before_pause = Some(MediaHeader::from_slice(&p[..MEDIA_HEADER_SIZE]).unwrap().sequence);
            }
        }

        let mut frame = tone_frame(100);
        let packet = capture.process(&mut frame, 42, 0).packet.expect("speech resumes");
        let seq_after = MediaHeader::from_slice(&packet[..MEDIA_HEADER_SIZE]).unwrap().sequence;
        assert_eq!(Some(seq_after.wrapping_sub(1)), last_seq_before_pause);
    }
}
