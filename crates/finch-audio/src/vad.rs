//! Voice Activity Detection (VAD) with a speech classifier, RMS dBFS thresholding and hangover frames.
//!
//! In [`VadMode::Speech`] a frame is speech only if the neural classifier says so *and* it is
//! above the user's dBFS threshold; in [`VadMode::Energy`] the threshold alone decides.
//! When volume drops, 5 hangover frames (100 ms) of speech are transmitted for smooth fading,
//! with the final packet tagged with the silence flag (`FLAG_SILENCE`).

use crate::resampler::FRAME_SIZE_48K;
use crate::speech::SpeechClassifier;

/// Default VAD threshold in dBFS (-45.0 dBFS).
pub const DEFAULT_VAD_THRESHOLD_DB: f32 = -45.0;

/// Number of hangover frames to send after speech ends (5 frames * 20 ms = 100 ms).
pub const HANGOVER_FRAME_COUNT: usize = 5;

/// VAD state decision for a 20 ms audio frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadDecision {
    /// Actively speaking (above threshold).
    Speaking,
    /// Hangover frame after speaking has ceased.
    /// `is_last` is true for the 5th hangover frame (signaling the receiver to flush jitter buffer).
    Hangover { is_last: bool },
    /// Silent frame (not transmitted to network).
    Silence,
}

impl VadDecision {
    /// True if this frame should be encoded and transmitted over QUIC.
    pub fn should_transmit(&self) -> bool {
        matches!(self, Self::Speaking | Self::Hangover { .. })
    }

    /// True if this is the final frame before silence (requires `FLAG_SILENCE`).
    pub fn is_silence_packet(&self) -> bool {
        matches!(self, Self::Hangover { is_last: true })
    }
}

/// How the VAD decides that a frame contains speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VadMode {
    /// Neural speech classifier (after noise suppression) plus the loudness threshold.
    /// Loud non-speech such as a fan or keyboard is not transmitted.
    #[default]
    Speech,
    /// Loudness threshold only. For synthetic signals (test tones, chirps) that are not speech.
    Energy,
}

/// Voice Activity Detector: speech classifier and/or root-mean-square (RMS) energy.
pub struct VoiceActivityDetector {
    threshold_db: f32,
    hangover_remaining: usize,
    was_speaking: bool,
    classifier: Option<SpeechClassifier>,
}

impl VoiceActivityDetector {
    /// Create a loudness-threshold VAD with the specified threshold in dBFS (e.g. -45.0).
    pub fn new(threshold_db: f32) -> Self {
        Self::with_mode(VadMode::Energy, threshold_db)
    }

    /// Create a VAD in the given mode. If the speech classifier cannot be created the VAD
    /// falls back to the loudness threshold instead of failing.
    pub fn with_mode(mode: VadMode, threshold_db: f32) -> Self {
        let classifier = match mode {
            VadMode::Energy => None,
            VadMode::Speech => match SpeechClassifier::new() {
                Ok(classifier) => Some(classifier),
                Err(error) => {
                    tracing::error!(%error, "Speech classifier unavailable; VAD uses loudness only");
                    None
                }
            },
        };
        Self {
            threshold_db,
            hangover_remaining: 0,
            was_speaking: false,
            classifier,
        }
    }

    /// Update the activation threshold in dBFS.
    pub fn set_threshold(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db;
    }

    /// Compute the RMS volume in dBFS for a 960-sample 48 kHz frame.
    pub fn compute_rms_dbfs(samples: &[f32; FRAME_SIZE_48K]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        let rms = (sum_sq / (samples.len() as f32)).sqrt();
        // Avoid log(0)
        20.0 * rms.max(1e-6).log10()
    }

    /// Process a 20 ms audio frame and determine voice activity.
    pub fn process(&mut self, samples: &[f32; FRAME_SIZE_48K]) -> VadDecision {
        let level_db = Self::compute_rms_dbfs(samples);
        // The classifier is streaming: it must see every frame, loud or not.
        let is_speech = match &mut self.classifier {
            Some(classifier) => classifier.is_speech(samples),
            None => true,
        };

        if is_speech && level_db >= self.threshold_db {
            self.was_speaking = true;
            self.hangover_remaining = HANGOVER_FRAME_COUNT;
            VadDecision::Speaking
        } else if self.hangover_remaining > 0 {
            self.hangover_remaining -= 1;
            let is_last = self.hangover_remaining == 0;
            if is_last {
                self.was_speaking = false;
            }
            VadDecision::Hangover { is_last }
        } else {
            self.was_speaking = false;
            VadDecision::Silence
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vad_active_and_hangover() {
        let mut vad = VoiceActivityDetector::new(-40.0);

        // Loud frame (0.1 amplitude => ~ -20 dBFS)
        let loud = [0.1f32; FRAME_SIZE_48K];
        let quiet = [0.0001f32; FRAME_SIZE_48K];

        assert_eq!(vad.process(&loud), VadDecision::Speaking);
        assert_eq!(vad.process(&loud), VadDecision::Speaking);

        // 5 hangover frames
        assert_eq!(vad.process(&quiet), VadDecision::Hangover { is_last: false });
        assert_eq!(vad.process(&quiet), VadDecision::Hangover { is_last: false });
        assert_eq!(vad.process(&quiet), VadDecision::Hangover { is_last: false });
        assert_eq!(vad.process(&quiet), VadDecision::Hangover { is_last: false });
        assert_eq!(vad.process(&quiet), VadDecision::Hangover { is_last: true });

        // Silence afterwards
        assert_eq!(vad.process(&quiet), VadDecision::Silence);
        assert_eq!(vad.process(&quiet), VadDecision::Silence);
    }
}
