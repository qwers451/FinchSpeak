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
//! - [`vad`] — RMS dBFS Voice Activity Detection with hangover
//! - [`mixer`] — Multi-speaker mixing with `tanh()` soft-limiter

pub mod devices;
pub mod jitter;
pub mod mixer;
pub mod opus;
pub mod resampler;
pub mod vad;

pub use devices::{list_input_devices, list_output_devices, AudioDeviceInfo};
pub use jitter::{JitterBuffer, PlayoutResult};
pub use mixer::AudioMixer;
pub use opus::{AudioOpusError, OpusVoiceDecoder, OpusVoiceEncoder};
pub use resampler::{InputResampler, OutputResampler, FRAME_SIZE_48K, INTERNAL_SAMPLE_RATE};
pub use vad::{VadDecision, VoiceActivityDetector};
