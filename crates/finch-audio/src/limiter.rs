//! Soft-knee limiter for the mixer output.
//!
//! Signals up to [`KNEE`] pass through untouched. Above it the level is bent smoothly towards,
//! but never reaching, full scale, so several loud speakers at once compress instead of clipping.

/// Level where the soft knee starts. Below it the signal is passed through bit-for-bit.
pub const KNEE: f32 = 0.8;

/// Width of the bend above the knee.
const SHOULDER: f32 = 0.2;

/// Largest `f32` below 1.0. The curve approaches 1.0 asymptotically, but `f32` tanh saturates to
/// exactly 1.0 for inputs well above the knee, so the output is capped here to keep `|y| < 1`.
const CEILING: f32 = 1.0 - f32::EPSILON / 2.0;

/// Soft-knee limiter applied to every mixed sample.
///
/// - `|x| <= 0.8`: returned unchanged.
/// - Above: `sign(x) * (0.8 + 0.2 * tanh((|x| - 0.8) / 0.2))`, capped just below full scale.
/// - `NaN` becomes `0.0`; infinities map to the ceiling of their sign.
///
/// Pure and allocation-free, so it is safe to call per sample on the audio path.
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    let magnitude = x.abs();
    if magnitude <= KNEE {
        return x;
    }
    let bent = KNEE + SHOULDER * ((magnitude - KNEE) / SHOULDER).tanh();
    let limited = bent.min(CEILING);
    if x.is_sign_negative() { -limited } else { limited }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inputs from -4.0 to 4.0 in 0.001 steps, including both ends.
    fn sweep() -> impl Iterator<Item = f32> {
        (-4000..=4000).map(|i| i as f32 / 1000.0)
    }

    #[test]
    fn quiet_signal_is_untouched() {
        for i in 0..960 {
            let x = (i as f32 * std::f32::consts::TAU / 48.0).sin() * 0.5;
            assert_eq!(soft_limit(x).to_bits(), x.to_bits(), "sample {i} changed");
        }
    }

    #[test]
    fn output_never_reaches_one() {
        for x in sweep() {
            let y = soft_limit(x);
            assert!(y.is_finite() && y.abs() < 1.0, "soft_limit({x}) = {y}");
        }
        assert!(soft_limit(4.0).abs() < 1.0);
        assert!(soft_limit(f32::INFINITY) < 1.0);
        assert!(soft_limit(f32::NEG_INFINITY) > -1.0);
    }

    #[test]
    fn limiter_is_monotonic() {
        let mut previous = f32::NEG_INFINITY;
        for x in sweep() {
            let y = soft_limit(x);
            assert!(y >= previous, "not monotonic at {x}: {previous} then {y}");
            previous = y;
        }
    }

    #[test]
    fn bend_follows_the_formula_above_the_knee() {
        assert_eq!(soft_limit(0.8), 0.8);
        let expected = 0.8 + 0.2 * 1.0f32.tanh();
        assert!((soft_limit(1.0) - expected).abs() < 1e-6, "got {}", soft_limit(1.0));
        assert!((soft_limit(-1.0) + expected).abs() < 1e-6);
    }

    #[test]
    fn nan_becomes_silence() {
        assert_eq!(soft_limit(f32::NAN), 0.0);
    }
}
