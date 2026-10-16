//! Natural microphone dynamics without look-ahead or sample buffering.
//!
//! AGC follows confirmed speech only. Compression preserves some vocal dynamics;
//! the peak limiter catches faster transients without adding a frame of delay.

use crate::capture::MAX_INPUT_GAIN;
use crate::resampler::FRAME_SIZE_48K;
use crate::vad::VadDecision;

const SAMPLE_RATE: f32 = 48_000.0;
const TARGET_DB: f32 = -22.0;
const MIN_GAIN_DB: f32 = -18.0;
const MAX_GAIN_DB: f32 = 12.0;
const AGC_UP_DB_PER_FRAME: f32 = 6.0 * 0.020;
const AGC_DOWN_DB_PER_FRAME: f32 = 18.0 * 0.020;
const THRESHOLD_DB: f32 = -18.0;
const RATIO: f32 = 3.0;
const KNEE_DB: f32 = 6.0;
const CEILING: f32 = 0.891_250_9; // -1 dBFS
const WET_STEP: f32 = 1.0 / FRAME_SIZE_48K as f32;

/// One microphone's adaptive gain, soft-knee compressor and instantaneous peak limiter.
/// All storage is fixed-size; processing allocates nothing and does no I/O.
#[derive(Debug)]
pub struct VoiceDynamics {
    enabled: bool,
    wet: f32,
    agc_db: f32,
    compressor_db: f32,
    limiter_gain: f32,
    compressor_attack: f32,
    compressor_release: f32,
    limiter_release: f32,
}

impl Default for VoiceDynamics {
    fn default() -> Self {
        Self::new()
    }
}

fn coefficient(seconds: f32) -> f32 {
    (-1.0 / (seconds * SAMPLE_RATE)).exp()
}
fn linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

fn compressor_reduction(level: f32) -> f32 {
    let over = level - THRESHOLD_DB;
    let slope = 1.0 / RATIO - 1.0;
    if over < -KNEE_DB * 0.5 {
        0.0
    } else if over > KNEE_DB * 0.5 {
        slope * over
    } else {
        slope * (over + KNEE_DB * 0.5).powi(2) / (2.0 * KNEE_DB)
    }
}

impl VoiceDynamics {
    pub fn new() -> Self {
        Self {
            enabled: true,
            wet: 1.0,
            agc_db: 0.0,
            compressor_db: 0.0,
            limiter_gain: 1.0,
            compressor_attack: coefficient(0.005),
            compressor_release: coefficient(0.120),
            limiter_release: coefficient(0.050),
        }
    }

    /// Crossfade on the next 20 ms frame; repeated setters do not reset adaptation.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if enabled {
            self.agc_db = 0.0;
            self.compressor_db = 0.0;
            self.limiter_gain = 1.0;
        }
    }

    /// Forget the old microphone/voice session while retaining the requested mode.
    pub fn reset(&mut self) {
        self.agc_db = 0.0;
        self.compressor_db = 0.0;
        self.limiter_gain = 1.0;
        self.wet = if self.enabled { 1.0 } else { 0.0 };
    }

    pub fn process(&mut self, frame: &mut [f32; FRAME_SIZE_48K], activity: VadDecision) {
        // A capture frame can have up to MAX_INPUT_GAIN headroom after manual trim.
        // Sanitize before squaring so malformed values cannot poison future state.
        for sample in frame.iter_mut() {
            *sample = if sample.is_finite() {
                sample.clamp(-MAX_INPUT_GAIN, MAX_INPUT_GAIN)
            } else {
                0.0
            };
        }
        if !self.enabled && self.wet == 0.0 {
            for sample in frame {
                *sample = sample.clamp(-1.0, 1.0);
            }
            return;
        }
        let previous_gain = linear(self.agc_db);
        if activity == VadDecision::Speaking {
            let mean_square =
                frame.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / FRAME_SIZE_48K as f64;
            if mean_square > 1e-8 {
                let level = (10.0 * mean_square.log10()) as f32;
                let desired = (TARGET_DB - level).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
                self.agc_db +=
                    (desired - self.agc_db).clamp(-AGC_DOWN_DB_PER_FRAME, AGC_UP_DB_PER_FRAME);
            }
        }
        let next_gain = linear(self.agc_db);
        let gain_step = (next_gain - previous_gain) / FRAME_SIZE_48K as f32;
        let mut gain = previous_gain;
        let target_wet = if self.enabled { 1.0 } else { 0.0 };
        for sample in frame {
            gain += gain_step;
            let input = *sample;
            let scaled = input * gain;
            let target_db = compressor_reduction(20.0 * scaled.abs().max(1e-6).log10());
            let smoothing = if target_db < self.compressor_db {
                self.compressor_attack
            } else {
                self.compressor_release
            };
            self.compressor_db = target_db + smoothing * (self.compressor_db - target_db);
            if self.compressor_db.abs() < 1e-6 {
                self.compressor_db = 0.0;
            }
            let compressed = scaled * linear(self.compressor_db);
            let target_limit = (CEILING / compressed.abs().max(1e-12)).min(1.0);
            self.limiter_gain = if target_limit < self.limiter_gain {
                target_limit
            } else {
                target_limit + self.limiter_release * (self.limiter_gain - target_limit)
            };
            let wet = compressed * self.limiter_gain;
            let delta = target_wet - self.wet;
            self.wet = if delta.abs() <= WET_STEP {
                target_wet
            } else {
                self.wet + delta.signum() * WET_STEP
            };
            let mixed = input.clamp(-1.0, 1.0) * (1.0 - self.wet) + wet * self.wet;
            *sample = if self.enabled || self.wet > 0.0 {
                mixed.clamp(-CEILING, CEILING)
            } else {
                mixed
            };
        }
        if !self.enabled && self.wet == 0.0 {
            self.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resampler::FRAME_SIZE_48K;
    use crate::vad::VadDecision;

    fn sine_frame(db: f32, index: usize) -> [f32; FRAME_SIZE_48K] {
        let amplitude = 2.0f32.sqrt() * 10f32.powf(db / 20.0);
        std::array::from_fn(|i| {
            amplitude
                * (std::f32::consts::TAU * 200.0 * (index * FRAME_SIZE_48K + i) as f32 / 48_000.0)
                    .sin()
        })
    }

    fn rms_db(samples: &[f32]) -> f32 {
        (10.0
            * (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64)
                .max(1e-12)
                .log10()) as f32
    }

    fn render_case(input_db: f32, seconds: usize, activity: VadDecision) -> (Vec<f32>, f32) {
        let mut dynamics = VoiceDynamics::new();
        let mut output = Vec::new();
        for i in 0..seconds * 50 {
            let mut frame = sine_frame(input_db, i);
            dynamics.process(&mut frame, activity);
            output.extend_from_slice(&frame);
        }
        let level = rms_db(&output[output.len() - 48_000..]);
        (output, level)
    }

    #[test]
    fn quiet_speech_is_raised() {
        let (out, level) = render_case(-38.0, 3, VadDecision::Speaking);
        assert!(level >= -32.0, "quiet speech stayed at {level} dBFS");
        let ceiling = 10f32.powf(-1.0 / 20.0) + 1e-6;
        assert!(out.iter().all(|s| s.is_finite() && s.abs() <= ceiling));
    }

    #[test]
    fn loud_speech_is_reduced() {
        let (_, level) = render_case(-12.0, 3, VadDecision::Speaking);
        assert!(level <= -18.0, "loud speech remained at {level} dBFS");
    }

    #[test]
    fn settled_speech_levels_converge() {
        let levels = [-38.0, -26.0, -12.0].map(|db| render_case(db, 3, VadDecision::Speaking).1);
        let low = levels.iter().copied().fold(f32::INFINITY, f32::min);
        let high = levels.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(high - low <= 8.0, "levels did not converge: {levels:?}");
    }

    #[test]
    fn gain_freezes_on_non_speech() {
        let mut dynamics = VoiceDynamics::new();
        for i in 0..100 {
            dynamics.process(&mut sine_frame(-38.0, i), VadDecision::Speaking);
        }
        let gain = dynamics.agc_db;
        for activity in [
            VadDecision::Silence,
            VadDecision::Hangover { is_last: false },
        ] {
            for i in 0..100 {
                dynamics.process(&mut sine_frame(-65.0, i), activity);
            }
            assert_eq!(dynamics.agc_db, gain, "pause changed the automatic gain");
        }
    }

    #[test]
    fn peaks_are_limited() {
        let mut dynamics = VoiceDynamics::new();
        for i in 0..150 {
            let mut frame = if i < 100 {
                sine_frame(-38.0, i)
            } else {
                std::array::from_fn(|n| if n % 2 == 0 { 8.0 } else { -8.0 })
            };
            dynamics.process(&mut frame, VadDecision::Speaking);
            assert!(frame
                .iter()
                .all(|s| s.is_finite() && s.abs() <= 10f32.powf(-1.0 / 20.0) + 1e-6));
        }
    }

    #[test]
    fn non_finite_samples_are_sanitized() {
        let mut dynamics = VoiceDynamics::new();
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            let mut frame = [value; FRAME_SIZE_48K];
            dynamics.process(&mut frame, VadDecision::Speaking);
            assert!(frame
                .iter()
                .all(|s| s.is_finite() && s.abs() <= 10f32.powf(-1.0 / 20.0) + 1e-6));
        }
        let mut valid = sine_frame(-26.0, 0);
        dynamics.process(&mut valid, VadDecision::Speaking);
        assert!(
            valid.iter().any(|s| s.abs() > 0.001),
            "invalid input poisoned future speech"
        );
    }

    #[test]
    fn zero_input_stays_zero() {
        let mut dynamics = VoiceDynamics::new();
        for _ in 0..100 {
            let mut frame = [0.0; FRAME_SIZE_48K];
            dynamics.process(&mut frame, VadDecision::Silence);
            assert_eq!(frame, [0.0; FRAME_SIZE_48K]);
        }
        assert_eq!(dynamics.agc_db, 0.0);
    }

    #[test]
    fn toggle_is_smoothed() {
        let mut dynamics = VoiceDynamics::new();
        for _ in 0..100 {
            dynamics.process(&mut [0.02; FRAME_SIZE_48K], VadDecision::Speaking);
        }
        let mut before = [0.02; FRAME_SIZE_48K];
        dynamics.process(&mut before, VadDecision::Speaking);
        dynamics.set_enabled(false);
        let mut transition = [0.02; FRAME_SIZE_48K];
        dynamics.process(&mut transition, VadDecision::Speaking);
        assert!((transition[0] - before[FRAME_SIZE_48K - 1]).abs() < 0.001);
        assert!((transition[FRAME_SIZE_48K - 1] - 0.02).abs() < 1e-6);
        assert!(transition.windows(2).all(|w| (w[1] - w[0]).abs() < 0.001));
        let mut bypass = [0.02; FRAME_SIZE_48K];
        dynamics.process(&mut bypass, VadDecision::Speaking);
        assert_eq!(bypass, [0.02; FRAME_SIZE_48K]);
        dynamics.set_enabled(true);
        let mut on = [0.02; FRAME_SIZE_48K];
        dynamics.process(&mut on, VadDecision::Speaking);
        assert!((on[0] - 0.02).abs() < 0.001);
    }

    #[test]
    fn reset_forgets_gain() {
        let mut dynamics = VoiceDynamics::new();
        for i in 0..100 {
            dynamics.process(&mut sine_frame(-38.0, i), VadDecision::Speaking);
        }
        dynamics.reset();
        let mut a = sine_frame(-26.0, 0);
        let mut b = a;
        dynamics.process(&mut a, VadDecision::Speaking);
        VoiceDynamics::new().process(&mut b, VadDecision::Speaking);
        assert_eq!(a, b);
    }

    #[test]
    fn sixty_seconds_processing_cost_is_recorded() {
        let mut dynamics = VoiceDynamics::new();
        let started = std::time::Instant::now();
        for i in 0..3_000 {
            dynamics.process(&mut sine_frame(-26.0, i), VadDecision::Speaking);
        }
        eprintln!(
            "DYNAMICS_METRIC: 60 s mono processed in {:?}",
            started.elapsed()
        );
    }
}
