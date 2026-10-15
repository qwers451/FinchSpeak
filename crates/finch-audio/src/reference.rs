//! Playout reference for echo cancellation.
//!
//! AEC needs the signal the speakers *actually played*, in playout order. The output device
//! callback runs at the device rate with irregular buffer sizes; this feeder turns those mono
//! samples into the 20 ms / 48 kHz frames [`crate::VoiceCapture::push_playback_reference`] expects.

use crate::resampler::{InputResampler, FRAME_SIZE_48K};

/// Converts mono device-rate playout samples into 48 kHz reference frames.
pub struct ReferenceFeeder {
    resampler: InputResampler,
    pending: Vec<f32>,
}

impl ReferenceFeeder {
    /// `device_rate` is the sample rate of the samples later passed to [`Self::push`].
    pub fn new(device_rate: u32) -> Result<Self, rubato::ResamplerConstructionError> {
        Ok(Self {
            resampler: InputResampler::new(device_rate)?,
            pending: Vec::new(),
        })
    }

    /// Append played samples (any amount); `sink` gets every completed 20 ms 48 kHz frame.
    pub fn push(&mut self, samples: &[f32], mut sink: impl FnMut(&[f32; FRAME_SIZE_48K])) {
        self.pending.extend_from_slice(samples);
        let mut frame = [0.0f32; FRAME_SIZE_48K];
        loop {
            let needed = self.resampler.input_frames_needed();
            if needed == 0 || self.pending.len() < needed {
                break;
            }
            match self.resampler.process(&self.pending[..needed], &mut frame) {
                Ok(()) => sink(&frame),
                Err(error) => tracing::error!(%error, "Playout reference resampling failed"),
            }
            self.pending.drain(..needed);
        }
    }

    /// Drop buffered samples (after the reference stream was interrupted).
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed `total` samples in irregular callback-sized pieces and count produced frames.
    fn frames_for(rate: u32, total: usize) -> usize {
        let mut feeder = ReferenceFeeder::new(rate).unwrap();
        let mut frames = 0;
        let mut fed = 0;
        let mut piece = 1;
        while fed < total {
            let n = piece.min(total - fed);
            feeder.push(&vec![0.1; n], |_| frames += 1);
            fed += n;
            piece = (piece * 7 + 13) % 997 + 1; // irregular sizes, like real callbacks
        }
        frames
    }

    #[test]
    fn test_48k_is_passthrough_in_20ms_frames() {
        let mut feeder = ReferenceFeeder::new(48_000).unwrap();
        let mut frames = Vec::new();
        let input: Vec<f32> = (0..FRAME_SIZE_48K * 2).map(|i| i as f32 / 4000.0).collect();
        feeder.push(&input[..500], |f| frames.push(*f));
        assert!(frames.is_empty());
        feeder.push(&input[500..], |f| frames.push(*f));
        assert_eq!(frames.len(), 2);
        assert_eq!(&frames[0][..], &input[..FRAME_SIZE_48K]);
        assert_eq!(&frames[1][..], &input[FRAME_SIZE_48K..]);
    }

    /// One second of played audio is 50 reference frames at every device rate.
    #[test]
    fn test_duration_is_preserved_at_all_device_rates() {
        for rate in [16_000u32, 44_100, 48_000, 96_000] {
            let frames = frames_for(rate, rate as usize);
            assert!(frames.abs_diff(50) <= 1, "{rate} Hz: {frames} frames for 1 s");
        }
    }

    #[test]
    fn test_clear_drops_partial_frame() {
        let mut feeder = ReferenceFeeder::new(48_000).unwrap();
        let mut frames = 0;
        feeder.push(&[0.5; 700], |_| frames += 1);
        feeder.clear();
        feeder.push(&[0.5; 700], |_| frames += 1);
        assert_eq!(frames, 0, "700 + 700 samples must not combine across a clear");
    }
}
