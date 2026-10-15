//! Audio resampling using `rubato`.
//!
//! Internal voice pipeline runs strictly at 48,000 Hz (960 samples per 20 ms frame).
//! This module converts between the native OS hardware rate (e.g. 44,100 Hz, 48,000 Hz, 96,000 Hz)
//! and the internal 48,000 Hz rate.

use rubato::{FftFixedInOut, Resampler};

/// Internal pipeline standard sample rate.
pub const INTERNAL_SAMPLE_RATE: u32 = 48_000;

/// Number of samples in a 20 ms frame at 48,000 Hz.
pub const FRAME_SIZE_48K: usize = 960;

/// Resampler converting native microphone input rate to 48,000 Hz.
pub enum InputResampler {
    /// Passthrough when hardware rate is already 48,000 Hz.
    Passthrough,
    /// Fast FFT resampler producing chunks of exactly 960 samples.
    Resampling {
        resampler: Box<FftFixedInOut<f32>>,
        input_needed: usize,
    },
}

impl InputResampler {
    /// Create a new input resampler from `source_rate` to 48,000 Hz.
    pub fn new(source_rate: u32) -> Result<Self, rubato::ResamplerConstructionError> {
        if source_rate == INTERNAL_SAMPLE_RATE {
            return Ok(Self::Passthrough);
        }

        // We want the resampler to output exactly FRAME_SIZE_48K (960) samples
        let resampler = FftFixedInOut::<f32>::new(
            source_rate as usize,
            INTERNAL_SAMPLE_RATE as usize,
            FRAME_SIZE_48K,
            1, // mono
        )?;
        let input_needed = resampler.input_frames_next();

        Ok(Self::Resampling {
            resampler: Box::new(resampler),
            input_needed,
        })
    }

    /// Number of input samples required for the next `process` call.
    pub fn input_frames_needed(&self) -> usize {
        match self {
            Self::Passthrough => FRAME_SIZE_48K,
            Self::Resampling { input_needed, .. } => *input_needed,
        }
    }

    /// Resample input samples to exactly 960 samples at 48 kHz.
    pub fn process(&mut self, input: &[f32], output: &mut [f32; FRAME_SIZE_48K]) -> Result<(), rubato::ResampleError> {
        match self {
            Self::Passthrough => {
                if input.len() >= FRAME_SIZE_48K {
                    output.copy_from_slice(&input[..FRAME_SIZE_48K]);
                }
                Ok(())
            }
            Self::Resampling { resampler, input_needed } => {
                let waves_in = [input];
                let waves_out = resampler.process(&waves_in, None)?;
                if let Some(channel) = waves_out.first() {
                    let to_copy = channel.len().min(FRAME_SIZE_48K);
                    output[..to_copy].copy_from_slice(&channel[..to_copy]);
                }
                *input_needed = resampler.input_frames_next();
                Ok(())
            }
        }
    }
}

/// Resampler converting internal 48,000 Hz frames (960 samples) to native speaker/headphone rate.
pub enum OutputResampler {
    /// Passthrough when target rate is 48,000 Hz.
    Passthrough,
    /// Fast FFT resampler taking 960 samples at 48 kHz and producing native samples.
    Resampling {
        resampler: Box<FftFixedInOut<f32>>,
    },
}

impl OutputResampler {
    /// Create a new output resampler from 48,000 Hz to `target_rate`.
    pub fn new(target_rate: u32) -> Result<Self, rubato::ResamplerConstructionError> {
        if target_rate == INTERNAL_SAMPLE_RATE {
            return Ok(Self::Passthrough);
        }

        // We input FRAME_SIZE_48K (960) samples at 48 kHz
        let target_chunk = (target_rate as usize * 20) / 1000;
        let resampler = FftFixedInOut::<f32>::new(
            INTERNAL_SAMPLE_RATE as usize,
            target_rate as usize,
            target_chunk,
            1, // mono
        )?;

        Ok(Self::Resampling {
            resampler: Box::new(resampler),
        })
    }

    /// Process a 960-sample 48 kHz mono frame into native output samples.
    pub fn process(&mut self, input: &[f32; FRAME_SIZE_48K]) -> Result<Vec<f32>, rubato::ResampleError> {
        match self {
            Self::Passthrough => Ok(input.to_vec()),
            Self::Resampling { resampler } => {
                let waves_in = [&input[..]];
                let waves_out = resampler.process(&waves_in, None)?;
                Ok(waves_out.into_iter().next().unwrap_or_default())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough() {
        let mut resampler = InputResampler::new(48_000).unwrap();
        assert_eq!(resampler.input_frames_needed(), 960);

        let input = vec![0.5f32; 960];
        let mut output = [0.0f32; 960];
        resampler.process(&input, &mut output).unwrap();
        assert_eq!(output[0], 0.5);
    }

    #[test]
    fn test_resample_44100_to_48000() {
        let mut resampler = InputResampler::new(44_100).unwrap();
        let needed = resampler.input_frames_needed();
        assert!(needed > 0);

        let input = vec![0.25f32; needed];
        let mut output = [0.0f32; 960];
        resampler.process(&input, &mut output).unwrap();
        assert_eq!(output.len(), 960);
    }

    #[test]
    fn test_output_resample_48000_to_44100() {
        let mut resampler = OutputResampler::new(44_100).unwrap();
        let input = [0.1f32; 960];
        let out = resampler.process(&input).unwrap();
        assert!(!out.is_empty());
    }
}
