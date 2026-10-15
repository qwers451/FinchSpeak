//! Acoustic Echo Cancellation (AEC).
//!
//! Stores the playback audio in a 960-sample reference ring buffer and cancels
//! acoustic coupling (echo) from the captured microphone signal using an adaptive filter
//! and correlated signal subtraction with soft limiting.

use crate::resampler::FRAME_SIZE_48K;

/// Maximum search lag for acoustic delay compensation (samples at 48 kHz).
/// 64 samples corresponds to ~1.33 ms (sufficient for direct speaker-to-mic path).
const MAX_AEC_LAG: usize = 64;

/// Default filter adaptation rate (learning rate for coupling updates).
const DEFAULT_ADAPTATION_RATE: f32 = 0.5;

/// Default maximum coupling ratio allowed (to avoid over-cancellation).
const DEFAULT_MAX_COUPLING: f32 = 1.0;

/// Acoustic Echo Canceller with a 960-sample reference ring buffer.
pub struct EchoCanceller {
    /// 960-sample circular buffer for reference (playback) audio.
    ref_ring_buf: [f32; FRAME_SIZE_48K],
    /// Current write position in the circular buffer.
    write_pos: usize,
    /// Whether any reference audio has been pushed.
    has_reference: bool,
    /// Estimated coupling gain (adaptive filter weight).
    filter_gain: f32,
    /// Adaptation rate for the adaptive filter.
    adaptation_rate: f32,
    /// Maximum allowed coupling factor.
    max_coupling: f32,
    /// Whether echo cancellation is enabled.
    enabled: bool,
}

impl Default for EchoCanceller {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoCanceller {
    /// Creates a new echo canceller with a 960-sample reference ring buffer.
    pub fn new() -> Self {
        Self {
            ref_ring_buf: [0.0f32; FRAME_SIZE_48K],
            write_pos: 0,
            has_reference: false,
            filter_gain: 0.0,
            adaptation_rate: DEFAULT_ADAPTATION_RATE,
            max_coupling: DEFAULT_MAX_COUPLING,
            enabled: true,
        }
    }

    /// Enable or disable acoustic echo cancellation.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns whether echo cancellation is currently active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Set adaptation rate (learning rate) in range `[0.0, 1.0]`.
    pub fn set_adaptation_rate(&mut self, rate: f32) {
        self.adaptation_rate = rate.clamp(0.0, 1.0);
    }

    /// Set maximum allowed coupling factor (default 1.0).
    pub fn set_max_coupling(&mut self, max: f32) {
        self.max_coupling = max.clamp(0.0, 2.0);
    }

    /// Returns current estimated filter coupling gain.
    pub fn filter_gain(&self) -> f32 {
        self.filter_gain
    }

    /// Reset internal filter state and circular buffer.
    pub fn reset(&mut self) {
        self.ref_ring_buf.fill(0.0);
        self.write_pos = 0;
        self.has_reference = false;
        self.filter_gain = 0.0;
    }

    /// Push a 20 ms reference frame (960 samples) of playback audio into the ring buffer.
    pub fn push_reference(&mut self, ref_frame: &[f32; FRAME_SIZE_48K]) {
        for &s in ref_frame {
            let val = if s.is_finite() { s } else { 0.0 };
            self.ref_ring_buf[self.write_pos] = val;
            self.write_pos = (self.write_pos + 1) % FRAME_SIZE_48K;
        }
        self.has_reference = true;
    }

    /// Push an individual reference sample into the ring buffer.
    pub fn push_reference_sample(&mut self, sample: f32) {
        let val = if sample.is_finite() { sample } else { 0.0 };
        self.ref_ring_buf[self.write_pos] = val;
        self.write_pos = (self.write_pos + 1) % FRAME_SIZE_48K;
        self.has_reference = true;
    }

    /// Read a sample from the reference ring buffer at a relative index and lag.
    #[inline]
    fn ref_at(&self, idx: usize, lag: usize) -> f32 {
        let pos = (self.write_pos + FRAME_SIZE_48K + idx - (lag % FRAME_SIZE_48K)) % FRAME_SIZE_48K;
        self.ref_ring_buf[pos]
    }

    /// Perform echo cancellation on a 20 ms capture frame (960 samples) in place.
    ///
    /// Subtracts the correlated echo signal estimated by the adaptive filter
    /// using the stored reference audio, applying soft limiting to preserve
    /// the local user's voice during double-talk.
    pub fn process_capture(&mut self, capture_frame: &mut [f32; FRAME_SIZE_48K]) {
        if !self.enabled || !self.has_reference {
            return;
        }

        // Sanitize capture inputs
        for s in capture_frame.iter_mut() {
            if !s.is_finite() {
                *s = 0.0;
            }
        }

        // Compute reference and capture signal energies
        let mut ref_energy = 0.0f32;
        let mut cap_energy = 0.0f32;
        for (i, &c) in capture_frame.iter().enumerate() {
            let r = self.ref_at(i, 0);
            ref_energy += r * r;
            cap_energy += c * c;
        }

        // If either reference or capture is silent, there is no echo to cancel
        if ref_energy < 1e-7 || cap_energy < 1e-7 {
            self.filter_gain *= 0.95;
            return;
        }

        // Find optimal lag tau with maximum cross-correlation
        let mut best_lag = 0;
        let mut best_corr = 0.0f32;
        for lag in 0..MAX_AEC_LAG {
            let mut corr = 0.0f32;
            for (i, &c) in capture_frame.iter().enumerate() {
                let r = self.ref_at(i, lag);
                corr += c * r;
            }
            if corr > best_corr {
                best_corr = corr;
                best_lag = lag;
            }
        }

        // If no positive correlation exists, signals are uncorrelated (no acoustic echo)
        if best_corr <= 0.0 {
            self.filter_gain *= 0.95;
            return;
        }

        // Energy of reference signal aligned to best_lag
        let mut ref_energy_lag = 0.0f32;
        for i in 0..FRAME_SIZE_48K {
            let r = self.ref_at(i, best_lag);
            ref_energy_lag += r * r;
        }

        if ref_energy_lag < 1e-7 {
            return;
        }

        // Normalized correlation coefficient rho in [0.0, 1.0]
        let rho = best_corr / ((ref_energy_lag * cap_energy).sqrt() + 1e-9);

        // Instantaneous optimal coupling projection factor
        let alpha_inst = best_corr / ref_energy_lag;
        let alpha_target = alpha_inst.clamp(0.0, self.max_coupling);

        // Double-talk detection:
        // When capture has significantly higher energy than reference and moderate correlation,
        // local speech is present; adapt slowly so local voice is not distorted.
        let is_double_talk = cap_energy > 2.5 * ref_energy_lag && rho < 0.75;

        let effective_rate = if is_double_talk {
            self.adaptation_rate * 0.1
        } else if rho > 0.65 {
            // High correlation (clear acoustic echo): fast adaptation
            self.adaptation_rate.max(0.8)
        } else {
            self.adaptation_rate
        };

        // Update adaptive filter gain
        self.filter_gain += effective_rate * (alpha_target - self.filter_gain);
        self.filter_gain = self.filter_gain.clamp(0.0, self.max_coupling);

        if self.filter_gain < 1e-5 {
            return;
        }

        // Subtract correlated echo signal with limiting
        for (i, c) in capture_frame.iter_mut().enumerate() {
            let r = self.ref_at(i, best_lag);
            let echo_est = self.filter_gain * r;
            let cancelled = *c - echo_est;

            // Clamping to avoid clipping / numeric instability
            *c = if cancelled.is_finite() {
                cancelled.clamp(-1.0, 1.0)
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
    fn test_aec_disabled_passthrough() {
        let mut aec = EchoCanceller::new();
        aec.set_enabled(false);
        assert!(!aec.is_enabled());

        let ref_frame = [0.5f32; FRAME_SIZE_48K];
        aec.push_reference(&ref_frame);

        let original = [0.3f32; FRAME_SIZE_48K];
        let mut capture = original;
        aec.process_capture(&mut capture);

        assert_eq!(capture, original);
    }

    #[test]
    fn test_aec_silent_reference() {
        let mut aec = EchoCanceller::new();
        let ref_frame = [0.0f32; FRAME_SIZE_48K];
        aec.push_reference(&ref_frame);

        let original = [0.25f32; FRAME_SIZE_48K];
        let mut capture = original;
        aec.process_capture(&mut capture);

        assert_eq!(capture, original);
    }

    #[test]
    fn test_aec_echo_suppression() {
        let mut aec = EchoCanceller::new();

        // Reference: synthetic tone
        let mut ref_frame = [0.0f32; FRAME_SIZE_48K];
        for (i, sample) in ref_frame.iter_mut().enumerate() {
            *sample = (i as f32 * 0.1).sin() * 0.5;
        }
        aec.push_reference(&ref_frame);

        // Capture: pure echo with 50% coupling
        let mut capture = [0.0f32; FRAME_SIZE_48K];
        for i in 0..FRAME_SIZE_48K {
            capture[i] = 0.5 * ref_frame[i];
        }

        let initial_energy: f32 = capture.iter().map(|&s| s * s).sum();
        assert!(initial_energy > 1.0);

        aec.process_capture(&mut capture);

        let final_energy: f32 = capture.iter().map(|&s| s * s).sum();

        // Echo should be attenuated by at least 90%
        assert!(final_energy < 0.1 * initial_energy);
    }

    #[test]
    fn test_aec_with_delay_lag() {
        let mut aec = EchoCanceller::new();

        let mut ref_frame = [0.0f32; FRAME_SIZE_48K];
        for (i, sample) in ref_frame.iter_mut().enumerate() {
            *sample = (i as f32 * 0.05).sin() * 0.4;
        }
        aec.push_reference(&ref_frame);

        // Capture: echo delayed by 10 samples
        let lag = 10;
        let mut capture = [0.0f32; FRAME_SIZE_48K];
        for i in lag..FRAME_SIZE_48K {
            capture[i] = 0.5 * ref_frame[i - lag];
        }

        let initial_energy: f32 = capture.iter().map(|&s| s * s).sum();
        assert!(initial_energy > 0.5);

        aec.process_capture(&mut capture);

        let final_energy: f32 = capture.iter().map(|&s| s * s).sum();
        assert!(final_energy < 0.2 * initial_energy);
    }

    #[test]
    fn test_aec_double_talk_preserves_speech() {
        let mut aec = EchoCanceller::new();

        let mut ref_frame = [0.0f32; FRAME_SIZE_48K];
        for (i, sample) in ref_frame.iter_mut().enumerate() {
            *sample = (i as f32 * 0.05).sin() * 0.2;
        }
        aec.push_reference(&ref_frame);

        // Capture: loud local speech + small echo
        let mut capture = [0.0f32; FRAME_SIZE_48K];
        for i in 0..FRAME_SIZE_48K {
            let local_speech = (i as f32 * 0.3).sin() * 0.8;
            let echo = 0.3 * ref_frame[i];
            capture[i] = local_speech + echo;
        }

        let speech_energy_before: f32 = capture.iter().map(|&s| s * s).sum();

        aec.process_capture(&mut capture);

        let energy_after: f32 = capture.iter().map(|&s| s * s).sum();

        // Local speech must remain strong (not silenced)
        assert!(energy_after > 0.5 * speech_energy_before);
    }

    #[test]
    fn test_aec_non_finite_inputs_safe() {
        let mut aec = EchoCanceller::new();
        let ref_frame = [f32::NAN; FRAME_SIZE_48K];
        aec.push_reference(&ref_frame);

        let mut capture = [f32::INFINITY; FRAME_SIZE_48K];
        aec.process_capture(&mut capture);

        for &s in &capture {
            assert!(s.is_finite());
            assert!((-1.0..=1.0).contains(&s));
        }
    }

    #[test]
    fn test_aec_sample_by_sample_push_and_reset() {
        let mut aec = EchoCanceller::new();
        for i in 0..FRAME_SIZE_48K {
            aec.push_reference_sample((i as f32 * 0.1).sin());
        }

        let mut capture = [0.0f32; FRAME_SIZE_48K];
        aec.process_capture(&mut capture);

        aec.reset();
        assert_eq!(aec.filter_gain(), 0.0);
    }
}
