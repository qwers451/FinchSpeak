//! # Screen Audio Loopback Capture and Opus Encoding
//!
//! Captures system audio loopback (e.g. WASAPI loopback on Windows) and encodes it
//! into high-fidelity stereo Opus packets for screen sharing sessions.
//!
//! ## Architecture
//!
//! - **CPAL Audio Stream**: Connects to the system audio output device in loopback mode.
//! - **Lock-Free Ring Buffer (`ringbuf`)**: Transmits raw PCM float samples from the CPAL audio
//!   callback to the processing thread without mutexes or memory allocations.
//! - **Audio Resampler & Channel Converter**: Downmixes or upmixes native device channels to stereo
//!   and resamples to 48,000 Hz using linear interpolation.
//! - **Opus Stereo Encoder (`audiopus`)**: Encodes 20 ms stereo frames (960 samples per channel,
//!   1920 total interleaved samples) at 96 kbps in Audio application mode.
//! - **Packet Packaging**: Prepends a strict 20-byte [`MediaHeader`] with [`MediaType::ScreenAudio`] (0x04)
//!   and passes [`MediaPacket`] datagrams to the receiver.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvError};
use std::sync::Arc;
use std::time::Duration;

use audiopus::coder::Encoder;
use audiopus::{Application, Bitrate, Channels, SampleRate};
use bytes::Bytes;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

use finch_protocol::media::{MediaPacket, MediaType};
use finch_protocol::{MediaHeader, MAX_DATAGRAM_PAYLOAD};

/// Standard sample rate for FinchSpeak audio pipeline (48,000 Hz).
pub const SAMPLE_RATE: u32 = 48_000;

/// Screen audio is captured and encoded in stereo (2 channels: Left and Right).
pub const CHANNELS: usize = 2;

/// Frame duration for each Opus audio packet (20 ms).
pub const FRAME_DURATION_MS: u32 = 20;

/// Number of audio samples per channel in a 20 ms frame at 48 kHz (960 samples).
pub const SAMPLES_PER_CHANNEL: usize = 960;

/// Total interleaved stereo samples in a 20 ms frame (960 * 2 = 1920 samples: L0, R0, L1, R1, ...).
pub const STEREO_FRAME_SAMPLES: usize = SAMPLES_PER_CHANNEL * CHANNELS;

/// Default Opus encoding bitrate for high-quality stereo screen audio (96 kbps).
pub const DEFAULT_BITRATE_BPS: i32 = 96_000;

/// Maximum payload capacity for an Opus encoded datagram (matches [`MAX_DATAGRAM_PAYLOAD`] = 1160 bytes).
pub const MAX_OPUS_PAYLOAD_SIZE: usize = MAX_DATAGRAM_PAYLOAD;

/// Capacity for the lock-free audio ring buffer (2 seconds of 48 kHz stereo audio = 192,000 samples).
pub const RINGBUF_CAPACITY: usize = 192_000;

/// Errors that can occur during system audio capture and Opus loopback encoding.
#[derive(Debug, thiserror::Error)]
pub enum LoopbackAudioError {
    #[error("Invalid frame size: expected {expected} samples, got {actual}")]
    InvalidFrameSize { expected: usize, actual: usize },

    #[error("No audio device available for loopback capture")]
    NoDeviceAvailable,

    #[error("Failed to query default stream config: {0}")]
    DefaultConfigError(#[from] cpal::DefaultStreamConfigError),

    #[error("Failed to build CPAL audio stream: {0}")]
    BuildStreamError(#[from] cpal::BuildStreamError),

    #[error("Failed to play CPAL audio stream: {0}")]
    PlayStreamError(#[from] cpal::PlayStreamError),

    #[error("Failed to spawn audio loopback processing thread: {0}")]
    ThreadSpawnError(#[from] std::io::Error),

    #[error("Opus encoder error: {0}")]
    Opus(#[from] audiopus::Error),

    #[error("Media header error: {0}")]
    MediaHeader(#[from] finch_protocol::media::MediaHeaderError),

    #[error("Unsupported audio sample format")]
    UnsupportedSampleFormat,

    #[error("Audio thread channel closed")]
    ChannelClosed,
}

/// Stereo Opus audio encoder for screen sharing audio.
///
/// Encodes 20 ms stereo frames (1920 interleaved samples) at 48 kHz
/// in [`Application::Audio`] mode at 96 kbps.
pub struct ScreenAudioEncoder {
    encoder: Encoder,
    output_buf: [u8; MAX_OPUS_PAYLOAD_SIZE],
}

impl ScreenAudioEncoder {
    /// Create a new stereo Opus encoder configured for 48 kHz high-quality audio.
    ///
    /// Bitrate is specified in kbps (e.g. 96 for 96,000 bps).
    pub fn new(bitrate_kbps: u32) -> Result<Self, LoopbackAudioError> {
        let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio)?;

        encoder.set_bitrate(Bitrate::BitsPerSecond((bitrate_kbps * 1000) as i32))?;
        encoder.set_inband_fec(true)?;
        encoder.set_packet_loss_perc(5)?;

        Ok(Self {
            encoder,
            output_buf: [0u8; MAX_OPUS_PAYLOAD_SIZE],
        })
    }

    /// Set encoder bitrate in kbps.
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<(), LoopbackAudioError> {
        self.encoder
            .set_bitrate(Bitrate::BitsPerSecond((bitrate_kbps * 1000) as i32))?;
        Ok(())
    }

    /// Encode a 20 ms stereo frame (960 samples per channel, 1920 interleaved samples).
    pub fn encode_stereo(
        &mut self,
        samples: &[f32; STEREO_FRAME_SAMPLES],
    ) -> Result<Bytes, LoopbackAudioError> {
        let len = self.encoder.encode_float(&samples[..], &mut self.output_buf)?;
        Ok(Bytes::copy_from_slice(&self.output_buf[..len]))
    }

    /// Encode a slice of stereo samples. Must have exactly `STEREO_FRAME_SAMPLES` elements.
    pub fn encode_stereo_slice(&mut self, samples: &[f32]) -> Result<Bytes, LoopbackAudioError> {
        if samples.len() != STEREO_FRAME_SAMPLES {
            return Err(LoopbackAudioError::InvalidFrameSize {
                expected: STEREO_FRAME_SAMPLES,
                actual: samples.len(),
            });
        }
        let len = self.encoder.encode_float(samples, &mut self.output_buf)?;
        Ok(Bytes::copy_from_slice(&self.output_buf[..len]))
    }

    /// Encode a 20 ms stereo frame and build a [`MediaPacket`] with [`MediaType::ScreenAudio`].
    pub fn packetize(
        &mut self,
        channel_id: u32,
        sender_peer_id: u32,
        sequence: u16,
        timestamp: u32,
        is_e2ee: bool,
        samples: &[f32; STEREO_FRAME_SAMPLES],
    ) -> Result<MediaPacket, LoopbackAudioError> {
        let payload = self.encode_stereo(samples)?;
        let mut flags = 0u8;
        if is_e2ee {
            flags |= finch_protocol::media::FLAG_E2EE;
        }

        let header = MediaHeader {
            media_type: MediaType::ScreenAudio,
            channel_id,
            sender_peer_id,
            sequence,
            timestamp,
            frame_id: sequence,
            fragment_index: 0,
            total_fragments: 1,
            flags,
        };

        Ok(MediaPacket::new(header, payload))
    }
}

/// Helper converter that takes raw audio samples at native sample rate and channel count,
/// and produces 48,000 Hz stereo interleaved samples without external heavy dependencies.
pub struct ResamplerConverter {
    source_rate: u32,
    source_channels: usize,
    /// Pending multi-channel samples if incoming slice length was not a multiple of channels.
    pending_channels: Vec<f32>,
    /// Accumulated stereo frames at source sample rate: (left, right).
    stereo_source_buf: Vec<(f32, f32)>,
    /// Accumulated stereo samples at 48,000 Hz: [L0, R0, L1, R1, ...].
    output_buf: Vec<f32>,
    /// Interpolation position counter.
    resample_pos: f64,
}

impl ResamplerConverter {
    /// Create a new channel converter and resampler from `source_rate` and `source_channels` to 48 kHz stereo.
    pub fn new(source_rate: u32, source_channels: usize) -> Self {
        Self {
            source_rate,
            source_channels: source_channels.max(1),
            pending_channels: Vec::with_capacity(source_channels.max(1)),
            stereo_source_buf: Vec::with_capacity(4096),
            output_buf: Vec::with_capacity(STEREO_FRAME_SAMPLES * 2),
            resample_pos: 0.0,
        }
    }

    /// Push incoming raw interleaved PCM samples from the capture stream.
    pub fn push_raw_samples(&mut self, samples: &[f32]) {
        let mut idx = 0;

        // If we had pending partial multi-channel frame from previous call:
        if !self.pending_channels.is_empty() {
            while idx < samples.len() && self.pending_channels.len() < self.source_channels {
                self.pending_channels.push(samples[idx]);
                idx += 1;
            }
            if self.pending_channels.len() == self.source_channels {
                let (l, r) = extract_stereo(&self.pending_channels, self.source_channels);
                self.stereo_source_buf.push((l, r));
                self.pending_channels.clear();
            }
        }

        // Process remaining complete multi-channel frames:
        while idx + self.source_channels <= samples.len() {
            let chunk = &samples[idx..idx + self.source_channels];
            let (l, r) = extract_stereo(chunk, self.source_channels);
            self.stereo_source_buf.push((l, r));
            idx += self.source_channels;
        }

        // Save any leftover partial frame samples:
        if idx < samples.len() {
            self.pending_channels.extend_from_slice(&samples[idx..]);
        }

        // Resample stereo_source_buf to 48,000 Hz:
        if self.source_rate == SAMPLE_RATE {
            // Direct 1:1 transfer
            for &(l, r) in &self.stereo_source_buf {
                self.output_buf.push(l);
                self.output_buf.push(r);
            }
            self.stereo_source_buf.clear();
        } else {
            let ratio = self.source_rate as f64 / SAMPLE_RATE as f64;
            while (self.resample_pos + 1.0) < self.stereo_source_buf.len() as f64 {
                let i = self.resample_pos.floor() as usize;
                let frac = (self.resample_pos - i as f64) as f32;
                let s0 = self.stereo_source_buf[i];
                let s1 = self.stereo_source_buf[i + 1];

                let l = (1.0 - frac) * s0.0 + frac * s1.0;
                let r = (1.0 - frac) * s0.1 + frac * s1.1;

                self.output_buf.push(l);
                self.output_buf.push(r);

                self.resample_pos += ratio;
            }

            let consumed = self.resample_pos.floor() as usize;
            if consumed > 0 {
                self.stereo_source_buf.drain(0..consumed);
                self.resample_pos -= consumed as f64;
            }
        }
    }

    /// Check if at least one complete 20 ms frame (1920 samples) is ready in the output buffer.
    pub fn has_frame(&self) -> bool {
        self.output_buf.len() >= STEREO_FRAME_SAMPLES
    }

    /// Pop a complete 20 ms frame (1920 samples) if ready.
    pub fn pop_frame(&mut self) -> Option<[f32; STEREO_FRAME_SAMPLES]> {
        if self.output_buf.len() < STEREO_FRAME_SAMPLES {
            return None;
        }

        let mut frame = [0.0f32; STEREO_FRAME_SAMPLES];
        frame.copy_from_slice(&self.output_buf[..STEREO_FRAME_SAMPLES]);
        self.output_buf.drain(0..STEREO_FRAME_SAMPLES);
        Some(frame)
    }
}

/// Downmix or extract stereo channels from a multi-channel frame.
fn extract_stereo(channels: &[f32], count: usize) -> (f32, f32) {
    match count {
        1 => (channels[0], channels[0]),
        _ => (channels[0], channels[1]),
    }
}

/// Configuration settings for the screen audio loopback capturer.
#[derive(Debug, Clone)]
pub struct ScreenAudioConfig {
    /// Target voice/media channel ID (u32 session id).
    pub channel_id: u32,
    /// Local sender peer ID (u32 session id).
    pub sender_peer_id: u32,
    /// Opus encoding bitrate in kbps (default: 96 kbps).
    pub bitrate_kbps: u32,
    /// Whether end-to-end encryption flag should be set on packets.
    pub is_e2ee: bool,
    /// Optional specific output device name to capture from. If `None`, defaults to the system default output.
    pub device_name: Option<String>,
}

impl Default for ScreenAudioConfig {
    fn default() -> Self {
        Self {
            channel_id: 0,
            sender_peer_id: 0,
            bitrate_kbps: 96,
            is_e2ee: false,
            device_name: None,
        }
    }
}

/// Enumerate all available audio output devices that can be captured via loopback.
pub fn list_loopback_devices() -> Result<Vec<String>, LoopbackAudioError> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.output_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
    }
    Ok(names)
}

/// Helper to search output and input devices by name.
fn find_device_by_name(host: &cpal::Host, target_name: &str) -> Option<cpal::Device> {
    if let Ok(devices) = host.output_devices() {
        for d in devices {
            if d.name().is_ok_and(|name| name == target_name) {
                return Some(d);
            }
        }
    }
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if d.name().is_ok_and(|name| name == target_name) {
                return Some(d);
            }
        }
    }
    None
}

/// System audio loopback capturer and Opus encoder.
///
/// Captures output audio via CPAL (WASAPI loopback on Windows), converts it to
/// 48 kHz stereo, encodes into 20 ms Opus packets (96 kbps), and provides
/// [`MediaPacket`] datagrams with [`MediaType::ScreenAudio`] and a 20-byte [`MediaHeader`].
pub struct ScreenAudioCapturer {
    config: ScreenAudioConfig,
    running: Arc<AtomicBool>,
    _stream: Option<cpal::Stream>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    receiver: Receiver<MediaPacket>,
}

/// Type alias for [`ScreenAudioCapturer`].
pub type AudioLoopback = ScreenAudioCapturer;

impl ScreenAudioCapturer {
    /// Start loopback capture and Opus encoding with the specified configuration.
    pub fn start(config: ScreenAudioConfig) -> Result<Self, LoopbackAudioError> {
        let host = cpal::default_host();
        let device = match &config.device_name {
            Some(name) => find_device_by_name(&host, name)
                .ok_or(LoopbackAudioError::NoDeviceAvailable)?,
            None => host
                .default_output_device()
                .or_else(|| host.default_input_device())
                .ok_or(LoopbackAudioError::NoDeviceAvailable)?,
        };

        // In WASAPI loopback, default_output_config() returns the output format of the device.
        let supported_config = device
            .default_output_config()
            .or_else(|_| device.default_input_config())
            .map_err(LoopbackAudioError::DefaultConfigError)?;

        let sample_format = supported_config.sample_format();
        let stream_config = supported_config.config();
        let source_rate = stream_config.sample_rate.0;
        let source_channels = stream_config.channels as usize;

        // Lock-free ring buffer for real-time CPAL callback to dedicated thread communication
        let rb = HeapRb::<f32>::new(RINGBUF_CAPACITY);
        let (mut prod, mut cons) = rb.split();

        let err_fn = move |err: cpal::StreamError| {
            tracing::error!("CPAL audio loopback stream error: {err}");
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = prod.push_slice(data);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    for &s in data {
                        let f = s as f32 / 32768.0;
                        let _ = prod.try_push(f);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    for &s in data {
                        let f = (s as f32 - 32768.0) / 32768.0;
                        let _ = prod.try_push(f);
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err(LoopbackAudioError::UnsupportedSampleFormat),
        };

        stream.play()?;

        let (tx, rx) = channel();
        let running = Arc::new(AtomicBool::new(true));
        let running_thread = Arc::clone(&running);
        let thread_config = config.clone();

        let thread_handle = std::thread::Builder::new()
            .name("finch-audio-loopback".to_string())
            .spawn(move || {
                let mut encoder = match ScreenAudioEncoder::new(thread_config.bitrate_kbps) {
                    Ok(enc) => enc,
                    Err(e) => {
                        tracing::error!("Failed to create ScreenAudioEncoder: {e}");
                        return;
                    }
                };

                let mut resampler = ResamplerConverter::new(source_rate, source_channels);
                let mut sequence: u16 = 0;
                let mut timestamp: u32 = 0;
                let mut temp_chunk = [0.0f32; 2048];

                while running_thread.load(Ordering::Relaxed) {
                    let read = cons.pop_slice(&mut temp_chunk);
                    if read > 0 {
                        resampler.push_raw_samples(&temp_chunk[..read]);

                        while resampler.has_frame() {
                            if let Some(stereo_frame) = resampler.pop_frame() {
                                match encoder.packetize(
                                    thread_config.channel_id,
                                    thread_config.sender_peer_id,
                                    sequence,
                                    timestamp,
                                    thread_config.is_e2ee,
                                    &stereo_frame,
                                ) {
                                    Ok(packet) => {
                                        sequence = sequence.wrapping_add(1);
                                        timestamp =
                                            timestamp.wrapping_add(SAMPLES_PER_CHANNEL as u32);
                                        if tx.send(packet).is_err() {
                                            // Receiver was dropped, exit processing thread
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to packetize loopback audio frame: {e}"
                                        );
                                    }
                                }
                            }
                        }
                    } else {
                        // Sleep briefly to avoid busy-spinning when no audio is playing
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            })
            .map_err(LoopbackAudioError::ThreadSpawnError)?;

        Ok(Self {
            config,
            running,
            _stream: Some(stream),
            thread_handle: Some(thread_handle),
            receiver: rx,
        })
    }

    /// Stop loopback capture and shutdown background processing thread.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        self._stream = None;
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }

    /// Check if audio capture is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Non-blocking receive for the next encoded audio media packet.
    pub fn try_recv(&self) -> Option<MediaPacket> {
        self.receiver.try_recv().ok()
    }

    /// Blocking receive for the next encoded audio media packet.
    pub fn recv(&self) -> Result<MediaPacket, RecvError> {
        self.receiver.recv()
    }

    /// Reference to the internal channel receiver.
    pub fn receiver(&self) -> &Receiver<MediaPacket> {
        &self.receiver
    }

    /// Current capture configuration.
    pub fn config(&self) -> &ScreenAudioConfig {
        &self.config
    }
}

impl Drop for ScreenAudioCapturer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_audio_encoder_creation() -> Result<(), LoopbackAudioError> {
        let mut encoder = ScreenAudioEncoder::new(96)?;
        encoder.set_bitrate(128)?;
        Ok(())
    }

    #[test]
    fn test_screen_audio_encode_stereo() -> Result<(), LoopbackAudioError> {
        let mut encoder = ScreenAudioEncoder::new(96)?;

        // Create 20 ms synthetic stereo sine wave (440 Hz Left, 880 Hz Right)
        let mut input = [0.0f32; STEREO_FRAME_SAMPLES];
        for i in 0..SAMPLES_PER_CHANNEL {
            let t = i as f32 / SAMPLE_RATE as f32;
            let left_sample = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
            let right_sample = (2.0 * std::f32::consts::PI * 880.0 * t).sin() * 0.5;
            input[i * 2] = left_sample;
            input[i * 2 + 1] = right_sample;
        }

        let encoded = encoder.encode_stereo(&input)?;
        assert!(!encoded.is_empty());
        assert!(encoded.len() <= MAX_OPUS_PAYLOAD_SIZE);
        Ok(())
    }

    #[test]
    fn test_screen_audio_packetize_and_parse() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = ScreenAudioEncoder::new(96)?;
        let input = [0.0f32; STEREO_FRAME_SAMPLES];

        let packet = encoder.packetize(42, 1001, 7, 6720, true, &input)?;

        // Validate 20-byte MediaHeader properties
        assert_eq!(packet.header.media_type, MediaType::ScreenAudio);
        assert_eq!(packet.header.channel_id, 42);
        assert_eq!(packet.header.sender_peer_id, 1001);
        assert_eq!(packet.header.sequence, 7);
        assert_eq!(packet.header.timestamp, 6720);
        assert_eq!(packet.header.frame_id, 7);
        assert_eq!(packet.header.fragment_index, 0);
        assert_eq!(packet.header.total_fragments, 1);
        assert!(packet.header.is_e2ee());
        assert!(!packet.header.is_silence());

        // Serialize packet and verify strict 20-byte header length + payload length
        let encoded_datagram = packet.encode();
        assert_eq!(
            encoded_datagram.len(),
            finch_protocol::MEDIA_HEADER_SIZE + packet.payload.len()
        );

        // Deserialize datagram back and verify round-trip integrity
        let parsed = MediaPacket::parse(encoded_datagram)?;
        assert_eq!(parsed.header, packet.header);
        assert_eq!(parsed.payload, packet.payload);

        Ok(())
    }

    #[test]
    fn test_screen_audio_invalid_frame_size() -> Result<(), LoopbackAudioError> {
        let mut encoder = ScreenAudioEncoder::new(96)?;
        let short_input = [0.0f32; 1000];

        let result = encoder.encode_stereo_slice(&short_input);
        match result {
            Err(LoopbackAudioError::InvalidFrameSize { expected, actual }) => {
                assert_eq!(expected, STEREO_FRAME_SAMPLES);
                assert_eq!(actual, 1000);
            }
            _ => panic!("Expected InvalidFrameSize error"),
        }
        Ok(())
    }

    #[test]
    fn test_resampler_converter_48k_passthrough() {
        let mut resampler = ResamplerConverter::new(48_000, 2);

        let input_samples = vec![0.1f32; STEREO_FRAME_SAMPLES];
        resampler.push_raw_samples(&input_samples);

        assert!(resampler.has_frame());
        let frame = resampler.pop_frame();
        assert!(frame.is_some());
        assert!(!resampler.has_frame());
    }

    #[test]
    fn test_resampler_converter_mono_to_stereo() {
        let mut resampler = ResamplerConverter::new(48_000, 1);

        // 960 mono samples -> 1920 stereo samples
        let mono_samples = vec![0.25f32; SAMPLES_PER_CHANNEL];
        resampler.push_raw_samples(&mono_samples);

        assert!(resampler.has_frame());
        let frame = resampler.pop_frame().unwrap();
        // Check that mono is duplicated to left and right
        assert_eq!(frame[0], 0.25);
        assert_eq!(frame[1], 0.25);
    }

    #[test]
    fn test_resampler_converter_44100_to_48000() {
        let mut resampler = ResamplerConverter::new(44_100, 2);

        // Feed enough 44.1 kHz stereo samples to produce multiple 48 kHz frames
        // At 44.1 kHz, 20 ms is 882 stereo samples (1764 floats)
        let samples = vec![0.5f32; 1764 * 3];
        resampler.push_raw_samples(&samples);

        assert!(resampler.has_frame());
        let frame = resampler.pop_frame();
        assert!(frame.is_some());
    }

    #[test]
    fn test_device_enumeration() {
        // Enumerate loopback devices without panic
        let _ = list_loopback_devices();
    }
}
