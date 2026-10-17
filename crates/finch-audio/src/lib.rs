//! # FinchSpeak Audio
//!
//! Audio pipeline for capture, noise suppression, Opus encoding/decoding,
//! jitter buffering, and playback.
//!
//! ## Modules
//!
//! - [`devices`] — Audio device enumeration via cpal
//! - [`resampler`] — Sample rate conversion via rubato
//! - [`opus`] — Opus VoIP encoder/decoder with PLC
//! - [`jitter`] — Adaptive jitter buffer with sequence rollover handling
//! - [`vad`] — Voice Activity Detection: neural speech classifier or RMS dBFS threshold, with hangover
//! - [`speech`] — Earshot speech classifier used by the VAD
//! - [`mixer`] — Multi-speaker mixing with `tanh()` soft-limiter
//! - [`capture`] — Voice capture pipeline (AEC → NS → VAD → Opus → packet)
//! - [`reference`] — playout reference feeder for echo cancellation
//! - [`aec`] — Acoustic Echo Cancellation with reference ring buffer
//! - [`noise`] — Neural network noise suppression based on `nnnoiseless`

pub mod aec;
pub mod capture;
pub mod devices;
pub mod jitter;
pub mod mixer;
pub mod noise;
pub mod opus;
pub mod quality;
pub mod reference;
pub mod resampler;
pub mod speech;
pub mod vad;

pub use aec::EchoCanceller;
pub use capture::{CaptureOutput, VoiceCapture, VoiceCaptureConfig, MAX_INPUT_GAIN, PREROLL_FRAMES};
pub use devices::{
    list_input_devices, list_output_devices, open_input_device, open_output_device, resolve_device_choice,
    AudioDeviceInfo, DeviceChoice,
};
pub use jitter::{JitterBuffer, PlayoutResult};
pub use mixer::{screen_audio_key, AudioMixer};
pub use noise::NoiseSuppressor;
pub use opus::{AudioOpusError, OpusVoiceDecoder, OpusVoiceEncoder};
pub use reference::ReferenceFeeder;
pub use resampler::{InputResampler, OutputResampler, FRAME_SIZE_48K, INTERNAL_SAMPLE_RATE};
pub use speech::SpeechClassifier;
pub use vad::DEFAULT_VAD_THRESHOLD_DB;
pub use vad::{VadDecision, VadMode, VoiceActivityDetector};

pub mod dynamics;
pub mod webrtc;
