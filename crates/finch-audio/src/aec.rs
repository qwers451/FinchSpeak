//! Acoustic echo cancellation using Sonora's pure Rust WebRTC AEC3 port.
//!
//! The public pipeline consumes 20 ms mono frames; AEC3 consumes two 10 ms
//! planar blocks at 48 kHz. Render history and acoustic delay estimation are
//! owned by AEC3, rather than a frame-local scalar correlation filter.
//!
//! Note: AEC3 is tuned for speech. Perfectly stationary or purely synthetic near-end signals
//! (pure tones, white noise) can be attenuated heavily in double talk; use speech to judge it.

use crate::resampler::FRAME_SIZE_48K;
use sonora::{AudioProcessing, Config, StreamConfig};

const BLOCK_SIZE: usize = FRAME_SIZE_48K / 2;

pub struct EchoCanceller {
    processor: AudioProcessing,
    /// Render-plus-capture device latency hint in ms (kept across `reset`).
    stream_delay_ms: i32,
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
        // Device buffering is unknown to this pure DSP adapter; until the caller supplies a
        // measured latency, AEC3 estimates the acoustic path delay itself.
        Self::with_delay(0)
    }

    fn with_delay(stream_delay_ms: i32) -> Self {
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
        if let Err(error) = processor.set_stream_delay_ms(stream_delay_ms) {
            tracing::warn!(%error, stream_delay_ms, "AEC3 rejected the stream delay; using its default");
        }
        Self {
            processor,
            stream_delay_ms,
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
        self.processor.set_stream_delay_ms(delay)?;
        self.stream_delay_ms = delay;
        Ok(())
    }

    /// Reset render history, delay estimation and the adaptive echo path.
    pub fn reset(&mut self) {
        let enabled = self.enabled;
        *self = Self::with_delay(self.stream_delay_ms);
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
        let (first, second) = frame.split_at_mut(BLOCK_SIZE);
        for block in [first, second] {
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

    /// Far-end surrogate: deterministic noise, so the echo path is excitable.
    fn far_end_frame(state: &mut u32) -> [f32; FRAME_SIZE_48K] {
        let mut frame = [0.0f32; FRAME_SIZE_48K];
        for sample in frame.iter_mut() {
            *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *sample = ((*state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.2;
        }
        frame
    }

    /// Echo-only capture (0.3 x far end): after two seconds of convergence the echo must be
    /// cancelled by at least 20 dB (the spec threshold).
    #[test]
    fn test_aec_cancels_echo_after_convergence() {
        let mut aec = EchoCanceller::new();
        let mut rng = 12_345u32;
        let (mut echo_energy, mut residual_energy) = (0.0f32, 0.0f32);
        for frame_index in 0..120 {
            let reference = far_end_frame(&mut rng);
            aec.push_reference(&reference);
            let mut capture = reference.map(|s| 0.3 * s);
            let energy_in: f32 = capture.iter().map(|&s| s * s).sum();
            aec.process_capture(&mut capture);
            if frame_index >= 100 {
                echo_energy += energy_in;
                residual_energy += capture.iter().map(|&s| s * s).sum::<f32>();
            }
        }
        let erle_db = 10.0 * (echo_energy / residual_energy.max(1e-12)).log10();
        assert!(erle_db >= 20.0, "ERLE only {erle_db:.1} dB");
    }

    // Double-talk quality is deliberately not unit-tested with synthetic signals: AEC3 attenuates
    // synthetic near-end stimuli by anywhere between ~1 dB and ~40 dB depending on details of
    // their shape (measured: harmonic+AM -0.7 dB, noise-like -9 dB, pure tone -44 dB), none of
    // which says anything about speech. The authoritative check runs real speech through the
    // whole chain: finch-testbed `echo_cancelled_after_convergence` (near speech must keep
    // within 6 dB).

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
