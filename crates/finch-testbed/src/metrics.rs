//! Signal metrics for voice quality checks.

/// Samples per 20 ms frame at 48 kHz.
const FRAME: usize = 960;

/// Mean power (mean square) of a signal.
pub fn power(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / x.len() as f64
}

/// RMS level in dBFS.
pub fn rms_dbfs(x: &[f32]) -> f32 {
    (10.0 * power(x).max(1e-12).log10()) as f32
}

/// Gain-invariant aligned residual SNR over fixed common voiced windows.
/// Fits one least-squares gain, so AGC amplification cannot masquerade as noise suppression.
/// Returns an error for empty/out-of-range/nonfinite or zero-energy reference windows.
pub fn gain_calibrated_snr_db(
    clean: &[f32],
    processed: &[f32],
    windows: &[std::ops::Range<usize>],
    lag: usize,
) -> anyhow::Result<f32> {
    anyhow::ensure!(!windows.is_empty(), "no common voiced windows");
    let (mut clean_energy, mut processed_energy, mut dot) = (0.0f64, 0.0f64, 0.0f64);
    for window in windows {
        anyhow::ensure!(
            window.start < window.end && window.end <= clean.len(),
            "invalid clean window"
        );
        let end = window
            .end
            .checked_add(lag)
            .ok_or_else(|| anyhow::anyhow!("alignment overflow"))?;
        anyhow::ensure!(end <= processed.len(), "processed window is missing");
        for (&c, &p) in clean[window.clone()]
            .iter()
            .zip(&processed[window.start + lag..end])
        {
            anyhow::ensure!(
                c.is_finite() && p.is_finite(),
                "nonfinite measurement sample"
            );
            clean_energy += f64::from(c).powi(2);
            processed_energy += f64::from(p).powi(2);
            dot += f64::from(c) * f64::from(p);
        }
    }
    anyhow::ensure!(
        clean_energy > 0.0 && processed_energy > 0.0,
        "no measurable signal energy"
    );
    let projected = dot * dot / clean_energy;
    let residual = (processed_energy - projected).max(processed_energy * 1e-12);
    Ok((10.0 * (projected.max(processed_energy * 1e-12) / residual).log10()) as f32)
}

/// SNR of a signal+noise segment given a noise-only segment:
/// `10·log10((P(signal_segment) − P(noise_segment)) / P(noise_segment))`.
pub fn snr_db(signal_segment: &[f32], noise_segment: &[f32]) -> f32 {
    let p_noise = power(noise_segment).max(1e-12);
    let p_signal = (power(signal_segment) - p_noise).max(1e-12);
    (10.0 * (p_signal / p_noise).log10()) as f32
}

/// Mean segmental SNR over 20 ms frames where `clean` is active (> −40 dBFS),
/// each frame clamped to [−10, 35] dB. `noise_floor` is the noise power.
pub fn segmental_snr_db(x: &[f32], clean: &[f32], noise_floor: f64) -> f32 {
    let floor = noise_floor.max(1e-12);
    let values: Vec<f64> = x
        .chunks(FRAME)
        .zip(clean.chunks(FRAME))
        .filter(|(_, c)| c.len() == FRAME && rms_dbfs(c) > -40.0)
        .map(|(f, _)| {
            let snr = 10.0 * ((power(f) - floor).max(1e-12) / floor).log10();
            snr.clamp(-10.0, 35.0)
        })
        .collect();
    if values.is_empty() {
        return f32::NAN;
    }
    (values.iter().sum::<f64>() / values.len() as f64) as f32
}

/// Minimum normalized cross-correlation for a lag to count as a match.
pub const MIN_DELAY_CORRELATION: f64 = 0.3;

/// Lag (in samples) of `reference` inside `recorded` by maximum cross-correlation.
///
/// Returns `None` when the best normalized correlation is below
/// [`MIN_DELAY_CORRELATION`] — e.g. `recorded` is silent or unrelated noise.
pub fn delay_samples(reference: &[f32], recorded: &[f32], max_lag: usize) -> Option<usize> {
    let ref_energy: f64 = reference.iter().map(|&r| r as f64 * r as f64).sum();
    if ref_energy <= 0.0 {
        return None;
    }
    let mut best: Option<(usize, f64)> = None;
    for lag in 0..=max_lag.min(recorded.len()) {
        let window = &recorded[lag..(lag + reference.len()).min(recorded.len())];
        let window_energy: f64 = window.iter().map(|&x| x as f64 * x as f64).sum();
        if window_energy <= 0.0 {
            continue;
        }
        let corr: f64 = reference
            .iter()
            .zip(window)
            .map(|(&r, &x)| r as f64 * x as f64)
            .sum();
        let normalized = corr / (ref_energy * window_energy).sqrt();
        if best.is_none_or(|(_, b)| normalized > b) {
            best = Some((lag, normalized));
        }
    }
    best.filter(|&(_, c)| c >= MIN_DELAY_CORRELATION)
        .map(|(lag, _)| lag)
}

/// Echo return loss enhancement: `10·log10(P(echo_in) / P(residual_out))`.
pub fn erle_db(echo_in: &[f32], residual_out: &[f32]) -> f32 {
    (10.0 * (power(echo_in).max(1e-12) / power(residual_out).max(1e-12)).log10()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{chirp, load_speech_48k, mix_at_snr, noise_scale_for, white_noise};

    fn tone(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn snr_of_known_mix() {
        let n = 48_000;
        let signal = tone(n);
        let noise = white_noise(2 * n, -30.0, 42);
        let mixed = mix_at_snr(&signal, &noise[..n], 5.0);
        // Same scaling applied to the noise-only segment.
        let scale = noise_scale_for(&signal, &noise[..n], 5.0);
        let noise_only: Vec<f32> = noise[n..].iter().map(|s| s * scale).collect();
        let snr = snr_db(&mixed, &noise_only);
        assert!((4.5..=5.5).contains(&snr), "snr {snr}");
    }

    #[test]
    fn delay_detected() {
        let reference = chirp(4_800, 300.0, 3_000.0);
        let mut recorded = vec![0.0f32; 2_400];
        recorded.extend_from_slice(&reference);
        recorded.extend(std::iter::repeat_n(0.0, 2_400));
        assert_eq!(delay_samples(&reference, &recorded, 10_000), Some(2_400));
    }

    #[test]
    fn delay_not_found_in_silence() {
        let reference = chirp(4_800, 300.0, 3_000.0);
        let recorded = vec![0.0f32; 20_000];
        assert_eq!(delay_samples(&reference, &recorded, 10_000), None);
    }

    #[test]
    fn delay_not_found_in_unrelated_noise() {
        let reference = chirp(4_800, 300.0, 3_000.0);
        let recorded = white_noise(20_000, -20.0, 5);
        assert_eq!(delay_samples(&reference, &recorded, 10_000), None);
    }

    #[test]
    fn erle_of_attenuated_copy() {
        let echo = tone(9_600);
        let residual: Vec<f32> = echo.iter().map(|s| s * 0.1).collect();
        let erle = erle_db(&echo, &residual);
        assert!((19.9..=20.1).contains(&erle), "erle {erle}");
    }

    #[test]
    fn fixtures_load_at_48k() {
        let speech = load_speech_48k("speech_1").unwrap();
        let expected = 5 * 48_000;
        assert!(
            speech.len().abs_diff(expected) <= 960,
            "len {}",
            speech.len()
        );
        assert!(rms_dbfs(&speech) > -35.0, "rms {}", rms_dbfs(&speech));
    }

    #[test]
    fn calibrated_snr_is_invariant_to_gain_and_handles_invalid_windows() {
        let clean = tone(48_000);
        let noise = white_noise(clean.len(), -30.0, 77);
        let mixed = mix_at_snr(&clean, &noise, 5.0);
        let windows: Vec<_> = (0..50).map(|i| i * 960..(i + 1) * 960).collect();
        let reference = gain_calibrated_snr_db(&clean, &mixed, &windows, 0).unwrap();
        assert!((4.5..=5.5).contains(&reference), "known SNR {reference}");
        for gain in [0.25, 4.0] {
            let scaled: Vec<_> = mixed.iter().map(|s| s * gain).collect();
            let actual = gain_calibrated_snr_db(&clean, &scaled, &windows, 0).unwrap();
            assert!((actual - reference).abs() < 0.1);
        }
        assert!(gain_calibrated_snr_db(&[], &[], &[], 0).is_err());
        assert!(gain_calibrated_snr_db(&[0.0; 960], &[0.0; 960], &windows[..1], 0).is_err());
        assert!(gain_calibrated_snr_db(&clean, &mixed, &windows, mixed.len()).is_err());
    }
}
