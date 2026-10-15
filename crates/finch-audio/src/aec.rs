//! Acoustic echo cancellation using Sonora's pure Rust WebRTC AEC3 port.
//!
//! The public pipeline consumes 20 ms mono frames; AEC3 consumes two 10 ms
//! planar blocks at 48 kHz. Render history and acoustic delay estimation are
//! owned by AEC3, rather than a frame-local scalar correlation filter.

use crate::resampler::FRAME_SIZE_48K;
use sonora::{AudioProcessing, Config, StreamConfig};

const BLOCK_SIZE: usize = FRAME_SIZE_48K / 2;

pub struct EchoCanceller {
    processor: AudioProcessing,
    render_block: [f32; BLOCK_SIZE],
    render_len: usize,
    has_reference: bool,
    enabled: bool,
}

impl Default for EchoCanceller {
    fn default() -> Self { Self::new() }
}

impl EchoCanceller {
    pub fn new() -> Self {
        let stream = StreamConfig::new(48_000, 1);
        let config = Config {
            echo_canceller: Some(sonora::config::EchoCanceller::default()),
            ..Config::default()
        };
        let mut processor = AudioProcessing::builder()
            .config(config)
            .capture_config(stream)
            .render_config(stream)
            .build();
        // Reference comes from actual playout. Device buffering is not known to
        // this pure DSP adapter; AEC3 estimates the acoustic path delay itself.
        processor.set_stream_delay_ms(0).expect("zero delay is valid");
        Self {
            processor,
            render_block: [0.0; BLOCK_SIZE],
            render_len: 0,
            has_reference: false,
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.reset();
            self.enabled = enabled;
        }
    }

    pub fn is_enabled(&self) -> bool { self.enabled }

    /// Supply measured render-device plus capture-device buffering latency.
    /// Sonora clamps values outside 0..=500 ms and returns a parameter warning.
    pub fn set_stream_delay_ms(&mut self, delay: i32) -> Result<(), sonora::Error> {
        self.processor.set_stream_delay_ms(delay)
    }
    /// Reset render history, delay estimation and the adaptive echo path.
    pub fn reset(&mut self) {
        let enabled = self.enabled;
        let delay = self.processor.stream_delay_ms();
        *self = Self::new();
        self.processor.set_stream_delay_ms(delay).expect("stored delay is valid");
        self.enabled = enabled;
    }

    /// Supply the mono signal actually played to speakers, in time order.
    pub fn push_reference(&mut self, frame: &[f32; FRAME_SIZE_48K]) {
        for &sample in frame { self.push_reference_sample(sample); }
    }

    /// Accumulate device-sized render callbacks into complete AEC3 blocks.
    pub fn push_reference_sample(&mut self, sample: f32) {
        self.render_block[self.render_len] = sanitize(sample);
        self.render_len += 1;
        if self.render_len == BLOCK_SIZE {
            let mut output = [0.0; BLOCK_SIZE];
            if let Err(error) = self.processor.process_render_f32(
                &[&self.render_block], &mut [&mut output],
            ) {
                tracing::error!(%error, "AEC3 render processing failed");
            }
            self.render_len = 0;
            self.has_reference |= self.render_block.iter().any(|s| s.abs() > 1e-7);
        }
    }

    /// Cancel echo on a 20 ms mono capture frame, retaining AEC3 state across calls.
    pub fn process_capture(&mut self, frame: &mut [f32; FRAME_SIZE_48K]) {
        if !self.enabled { return; }
        for sample in frame.iter_mut() { *sample = sanitize(*sample); }
        // Keep the AEC clock moving through silent render preroll too.
        for block in frame.chunks_exact_mut(BLOCK_SIZE) {
            let mut output = [0.0; BLOCK_SIZE];
            match self.processor.process_capture_f32(&[block], &mut [&mut output]) {
                Ok(()) if self.has_reference => block.copy_from_slice(&output),
                Ok(()) => {},
                Err(error) => tracing::error!(%error, "AEC3 capture processing failed"),
            }
        }
    }
}

fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 }
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

        // AEC3 has a filter-bank startup delay. Evaluate double talk after the
        // same two-second echo-only convergence period as the quality fixture.
        for _ in 0..100 {
            aec.push_reference(&ref_frame);
            let mut echo = ref_frame.map(|sample| sample * 0.3);
            aec.process_capture(&mut echo);
        }
        let mixed = capture;
        for _ in 0..10 {
            aec.push_reference(&ref_frame);
            capture = mixed;
            aec.process_capture(&mut capture);
        }
        capture = mixed;
        aec.push_reference(&ref_frame);
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
        let original = [0.25; FRAME_SIZE_48K];
        capture = original;
        aec.process_capture(&mut capture);
        assert_eq!(capture, original, "reset must discard the render/echo history");
    }
}





