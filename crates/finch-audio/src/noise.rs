//! Neural network noise suppression based on `nnnoiseless` (RNNoise).
//!
//! Processes 20 ms frames (960 samples at 48 kHz).
//! Because `nnnoiseless::DenoiseState` operates on 10 ms windows (480 samples),
//! each 960-sample frame is split into two contiguous 480-sample subframes and
//! processed sequentially in place without heap allocations.

use nnnoiseless::DenoiseState;

use crate::resampler::FRAME_SIZE_48K;

/// Subframe size expected by the underlying RNNoise model (10 ms at 48 kHz).
pub const NNNOISELESS_FRAME_SIZE: usize = DenoiseState::FRAME_SIZE;

/// PCM amplitude scaling factor: `nnnoiseless` was trained on 16-bit integer PCM values
/// in `[-32768.0, 32767.0]`. Floating point samples in FinchSpeak operate in `[-1.0, 1.0]`.
const PCM_16_SCALE: f32 = 32767.0;

/// Neural network noise suppressor based on `nnnoiseless`.
pub struct NoiseSuppressor {
    state: Box<DenoiseState<'static>>,
    scratch: [f32; NNNOISELESS_FRAME_SIZE],
    enabled: bool,
}

impl Default for NoiseSuppressor {
    fn default() -> Self {
        Self::new()
    }
}

impl NoiseSuppressor {
    /// Creates a new noise suppressor, enabled by default.
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            scratch: [0.0f32; NNNOISELESS_FRAME_SIZE],
            enabled: true,
        }
    }

    /// Enable or disable noise suppression.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns whether noise suppression is currently active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Reset internal RNN filter state.
    pub fn reset(&mut self) {
        self.state = DenoiseState::new();
        self.scratch.fill(0.0);
    }

    /// Process a 20 ms frame (960 samples at 48 kHz) in place.
    ///
    /// The 960 samples are divided into two 480-sample subframes and processed
    /// sequentially using an internal scratch buffer without heap allocations.
    pub fn process_frame(&mut self, frame: &mut [f32; FRAME_SIZE_48K]) {
        if !self.enabled {
            return;
        }

        let (first, second) = frame.split_at_mut(NNNOISELESS_FRAME_SIZE);
        self.process_subframe(first);
        self.process_subframe(second);
    }

    /// Process a single 480-sample subframe in place without heap allocations.
    fn process_subframe(&mut self, subframe: &mut [f32]) {
        if subframe.len() != NNNOISELESS_FRAME_SIZE {
            return;
        }

        // Copy and scale from normalized [-1.0, 1.0] to [-32767.0, 32767.0]
        for (dst, &src) in self.scratch.iter_mut().zip(subframe.iter()) {
            let val = if src.is_finite() { src } else { 0.0 };
            *dst = val * PCM_16_SCALE;
        }

        // Denoise using nnnoiseless
        self.state.process_frame(subframe, &self.scratch);

        // Rescale back to [-1.0, 1.0] and clamp to prevent clipping
        for s in subframe.iter_mut() {
            let rescaled = *s / PCM_16_SCALE;
            *s = if rescaled.is_finite() {
                rescaled.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_suppressor_disabled_passthrough() {
        let mut suppressor = NoiseSuppressor::new();
        suppressor.set_enabled(false);
        assert!(!suppressor.is_enabled());

        let original = [0.42f32; FRAME_SIZE_48K];
        let mut frame = original;

        suppressor.process_frame(&mut frame);
        assert_eq!(frame, original);
    }

    #[test]
    fn test_noise_suppressor_silence() {
        let mut suppressor = NoiseSuppressor::new();
        assert!(suppressor.is_enabled());

        let mut frame = [0.0f32; FRAME_SIZE_48K];
        suppressor.process_frame(&mut frame);

        for &sample in &frame {
            assert!(sample.abs() < 1e-4);
        }
    }

    #[test]
    fn test_noise_suppressor_suppresses_noise_without_panics() {
        let mut suppressor = NoiseSuppressor::new();

        // Feed multiple frames of noisy audio (e.g. low-amplitude high-frequency noise)
        // RNNoise warms up over a few frames.
        let mut frame = [0.0f32; FRAME_SIZE_48K];
        for (i, sample) in frame.iter_mut().enumerate() {
            // Simulated noisy wave
            *sample = if i % 2 == 0 { 0.05 } else { -0.05 };
        }

        for _ in 0..10 {
            suppressor.process_frame(&mut frame);
            for &s in &frame {
                assert!(s.is_finite());
                assert!((-1.0..=1.0).contains(&s));
            }
        }
    }

    #[test]
    fn test_noise_suppressor_extreme_values_safe() {
        let mut suppressor = NoiseSuppressor::new();
        let mut frame = [f32::NAN; FRAME_SIZE_48K];
        frame[0] = f32::INFINITY;
        frame[1] = f32::NEG_INFINITY;
        frame[2] = 1000.0;
        frame[3] = -1000.0;

        // Must not panic on invalid or non-finite inputs
        suppressor.process_frame(&mut frame);

        for &s in &frame {
            assert!(s.is_finite());
            assert!((-1.0..=1.0).contains(&s));
        }
    }

    #[test]
    fn test_noise_suppressor_reset() {
        let mut suppressor = NoiseSuppressor::new();
        let mut frame = [0.1f32; FRAME_SIZE_48K];
        suppressor.process_frame(&mut frame);
        suppressor.reset();
        assert!(suppressor.is_enabled());
    }
}
