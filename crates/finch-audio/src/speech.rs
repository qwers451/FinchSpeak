//! Neural speech classifier for voice activity detection.
//!
//! A loudness threshold cannot tell a fan from a voice (AUD-050). This wraps Earshot, a small
//! pure-Rust speech detector that works on 16 kHz / 16 ms blocks, behind the pipeline's
//! 48 kHz / 20 ms frames. It runs *after* noise suppression.

use earshot::Detector;

use crate::resampler::{OutputResampler, FRAME_SIZE_48K};

/// Earshot consumes exactly 256 samples (16 ms at 16 kHz) per call.
const EARSHOT_BLOCK: usize = 256;

/// The model's documented default decision threshold.
pub const SPEECH_SCORE_THRESHOLD: f32 = 0.5;

pub struct SpeechClassifier {
    detector: Box<Detector>,
    to_16k: OutputResampler,
    pending: Vec<f32>,
}

impl SpeechClassifier {
    pub fn new() -> Result<Self, rubato::ResamplerConstructionError> {
        Ok(Self {
            detector: Detector::default_boxed(),
            to_16k: OutputResampler::new(16_000)?,
            pending: Vec::with_capacity(2 * EARSHOT_BLOCK),
        })
    }

    /// Highest speech score (0..=1) of the blocks completed by this 20 ms frame.
    ///
    /// Every frame completes at least one 256-sample block (320 new samples per call), so the
    /// classifier always has fresh evidence. On an internal resampling error the frame counts as
    /// speech, so a failure never silences the microphone.
    pub fn score(&mut self, frame: &[f32; FRAME_SIZE_48K]) -> f32 {
        match self.to_16k.process(frame) {
            Ok(samples) => self.pending.extend_from_slice(&samples),
            Err(error) => {
                tracing::error!(%error, "Speech classifier resampling failed; assuming speech");
                return 1.0;
            }
        }

        let mut best = 0.0f32;
        let mut consumed = 0;
        while self.pending.len() - consumed >= EARSHOT_BLOCK {
            let block = &self.pending[consumed..consumed + EARSHOT_BLOCK];
            best = best.max(self.detector.predict_f32(block));
            consumed += EARSHOT_BLOCK;
        }
        self.pending.drain(..consumed);
        best
    }

    pub fn is_speech(&mut self, frame: &[f32; FRAME_SIZE_48K]) -> bool {
        self.score(frame) >= SPEECH_SCORE_THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silence_is_not_speech() {
        let mut classifier = SpeechClassifier::new().unwrap();
        for _ in 0..50 {
            assert!(!classifier.is_speech(&[0.0; FRAME_SIZE_48K]));
        }
    }

    /// 320 new samples per frame always complete a 256-sample block.
    #[test]
    fn test_every_frame_is_scored() {
        let mut classifier = SpeechClassifier::new().unwrap();
        for _ in 0..200 {
            classifier.score(&[0.0; FRAME_SIZE_48K]);
            assert!(classifier.pending.len() < EARSHOT_BLOCK, "leftover grows without bound");
        }
    }
}
