//! Full production capture, fixed common voiced windows and A/B artifacts.
use finch_audio::{FRAME_SIZE_48K, VoiceCapture, VoiceCaptureConfig};
use finch_testbed::{
    audio::{load_speech_48k, mix_at_snr, noise_scale_for, pink_noise, white_noise, write_wav_48k},
    metrics::{delay_samples, gain_calibrated_snr_db, rms_dbfs},
};
use std::{ops::Range, path::PathBuf};
const SR: usize = 48_000;
const LAG: usize = 480; // Calibrated RNNoise overlap; the gain-only APM introduces no frame queue.
fn artifacts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/voice-enhancement-out")
}
fn capture(input: &[f32], enabled: bool) -> anyhow::Result<Vec<f32>> {
    let mut capture = VoiceCapture::new(VoiceCaptureConfig {
        voice_enhancement: enabled,
        ..Default::default()
    })?;
    let mut output = Vec::with_capacity(input.len() + FRAME_SIZE_48K);
    for index in 0..input.len().div_ceil(FRAME_SIZE_48K) + 1 {
        let mut frame = [0.0; FRAME_SIZE_48K];
        let start = index * FRAME_SIZE_48K;
        if start < input.len() {
            let end = (start + FRAME_SIZE_48K).min(input.len());
            frame[..end - start].copy_from_slice(&input[start..end]);
        }
        capture.process(&mut frame, 1, index as u32 * 20)?;
        let ceiling = if enabled { 0.891252 } else { 1.000001 };
        assert!(frame.iter().all(|s| s.is_finite() && s.abs() <= ceiling));
        output.extend_from_slice(&frame);
    }
    Ok(output)
}
fn repeated(name: &str) -> anyhow::Result<Vec<f32>> {
    let source = load_speech_48k(name)?;
    Ok(source.iter().copied().cycle().take(15 * SR).collect())
}
fn windows(clean: &[f32], offset: usize) -> Vec<Range<usize>> {
    clean
        .as_chunks::<FRAME_SIZE_48K>()
        .0
        .iter()
        .enumerate()
        .filter(|(index, frame)| *index >= 500 && rms_dbfs(&frame[..]) > -40.0)
        .map(|(index, _)| offset + index * FRAME_SIZE_48K..offset + (index + 1) * FRAME_SIZE_48K)
        .collect()
}
fn window_level(samples: &[f32], windows: &[Range<usize>], lag: usize) -> f32 {
    let (mut energy, mut count) = (0.0f64, 0usize);
    for window in windows {
        for sample in &samples[window.start + lag..window.end + lag] {
            energy += f64::from(*sample).powi(2);
            count += 1;
        }
    }
    (10.0 * (energy / count.max(1) as f64).max(1e-12).log10()) as f32
}
#[test]
fn rnnoise_alignment_is_fixed_before_ab_comparison() -> anyhow::Result<()> {
    let input = load_speech_48k("speech_1")?;
    let output = capture(&input, false)?;
    let lag = delay_samples(&input[..SR], &output[..SR + 960], 960).expect("speech alignment");
    assert_eq!(
        lag, LAG,
        "do not optimize different alignments to favor an A/B run"
    );
    Ok(())
}
#[test]
fn three_voices_converge_without_clipping_or_losing_quiet_speech() -> anyhow::Result<()> {
    let mut report = Vec::new();
    let mut verdicts = Vec::new();
    for name in ["speech_1", "speech_2", "speech_3"] {
        let source = repeated(name)?;
        let common = windows(&source, 0);
        assert!(common.len() > 20);
        let mut off_levels = Vec::new();
        let mut on_levels = Vec::new();
        for target in [-38.0, -26.0, -12.0] {
            let gain = 10f32.powf((target - rms_dbfs(&source)) / 20.0);
            let clipped = source.iter().filter(|s| (**s * gain).abs() > 1.0).count();
            let input: Vec<_> = source.iter().map(|s| (s * gain).clamp(-1.0, 1.0)).collect();
            let off = capture(&input, false)?;
            let on = capture(&input, true)?;
            let off_db = window_level(&off, &common, LAG);
            let on_db = window_level(&on, &common, LAG);
            let off_snr = gain_calibrated_snr_db(&input, &off, &common, LAG)?;
            let on_snr = gain_calibrated_snr_db(&input, &on, &common, LAG)?;
            eprintln!(
                "ENHANCEMENT {name} input={target:.0} off={off_db:.2} on={on_db:.2} gain={:.2} source_clipped={clipped} calibrated_residual_snr_off={off_snr:.2} on={on_snr:.2}",
                on_db - off_db
            );
            let label = format!("{name}_{target:.0}");
            for (suffix, samples) in [("input", &input), ("off", &off), ("on", &on)] {
                write_wav_48k(&artifacts().join(format!("{label}_{suffix}.wav")), samples)?;
            }
            report.push(serde_json::json!({"fixture":name,"input_target_db":target,"off_db":off_db,"on_db":on_db,"source_clipped_samples":clipped,"residual_snr_off":off_snr,"residual_snr_on":on_snr,"common_windows":common.len(),"adaptation_seconds":10,"measured_seconds":5}));
            off_levels.push(off_db);
            on_levels.push(on_db);
        }
        let span = |x: &[f32]| {
            x.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - x.iter().copied().fold(f32::INFINITY, f32::min)
        };
        verdicts.push((
            name,
            on_levels[0] - off_levels[0],
            span(&off_levels) - span(&on_levels),
            on_levels[2] - off_levels[2],
        ));
    }
    std::fs::write(
        artifacts().join("levels.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    for (name, quiet_boost, span_reduction, loud_boost) in verdicts {
        assert!(
            quiet_boost >= 6.0,
            "{name}: quiet boost {quiet_boost:.2} <6 dB"
        );
        assert!(
            span_reduction >= 6.0,
            "{name}: level spread reduction {span_reduction:.2} <6 dB"
        );
        assert!(
            loud_boost <= 3.0,
            "{name}: loud source increased {loud_boost:.2} >3 dB"
        );
    }
    Ok(())
}
#[test]
fn stationary_noise_ab_uses_the_same_voiced_windows_and_gain_calibration() -> anyhow::Result<()> {
    let mut report = Vec::new();
    for name in ["speech_1", "speech_2", "speech_3"] {
        let mut clean = repeated(name)?;
        let scale = 10f32.powf((-26.0 - rms_dbfs(&clean)) / 20.0);
        clean.iter_mut().for_each(|s| *s *= scale);
        let common = windows(&clean, SR);
        for kind in ["white", "pink"] {
            let noise = if kind == "white" {
                white_noise(clean.len() + SR, -30.0, 42)
            } else {
                pink_noise(clean.len() + SR, -30.0, 42)
            };
            let noise_gain = noise_scale_for(&clean, &noise[SR..], 5.0);
            let mut input: Vec<_> = noise[..SR].iter().map(|s| s * noise_gain).collect();
            input.extend(mix_at_snr(&clean, &noise[SR..], 5.0));
            let mut reference = vec![0.0; SR];
            reference.extend_from_slice(&clean);
            let off = capture(&input, false)?;
            let on = capture(&input, true)?;
            let before = gain_calibrated_snr_db(&reference, &input, &common, 0)?;
            let control = gain_calibrated_snr_db(&reference, &off, &common, LAG)?;
            let enhanced = gain_calibrated_snr_db(&reference, &on, &common, LAG)?;
            eprintln!(
                "ENHANCEMENT_NOISE {name} {kind} residual_snr input={before:.2} off={control:.2} on={enhanced:.2}"
            );
            report.push(serde_json::json!({"fixture":name,"noise":kind,"input":before,"off":control,"on":enhanced}));
            for (suffix, samples) in [("input", &input), ("off", &off), ("on", &on)] {
                write_wav_48k(
                    &artifacts().join(format!("{name}_{kind}_{suffix}.wav")),
                    samples,
                )?;
            }
        }
    }
    std::fs::write(
        artifacts().join("stationary.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
#[test]
fn pauses_do_not_raise_the_noise_floor_and_zero_trim_mutes_immediately() -> anyhow::Result<()> {
    let source = repeated("speech_1")?;
    for kind in ["pink", "white"] {
        let mut capture = VoiceCapture::new(VoiceCaptureConfig::default())?;
        let mut output = Vec::new();
        let mut input = source[..6 * SR].to_vec();
        input.extend(if kind == "pink" {
            pink_noise(2 * SR, -45.0, 71)
        } else {
            white_noise(2 * SR, -45.0, 71)
        });
        input.extend(vec![0.0; SR]);
        input.extend_from_slice(&source[..3 * SR]);
        input.extend(vec![0.0; FRAME_SIZE_48K]);
        for (i, chunk) in input.as_chunks::<FRAME_SIZE_48K>().0.iter().enumerate() {
            let mut frame = *chunk;
            capture.process(&mut frame, 1, i as u32 * 20)?;
            output.extend_from_slice(&frame);
        }
        let first = rms_dbfs(&output[6 * SR + LAG..7 * SR + LAG]);
        let last = rms_dbfs(&output[7 * SR + LAG..8 * SR + LAG]);
        eprintln!("ENHANCEMENT_PAUSE {kind} noise_first={first:.2} noise_last={last:.2}");
        assert!(
            last <= first + 3.0,
            "noise floor grew {first:.2} -> {last:.2}"
        );
        capture.set_input_gain(0.0)?;
        let mut frame = [0.3; FRAME_SIZE_48K];
        capture.process(&mut frame, 1, 99_000)?;
        assert!(frame.iter().all(|s| *s == 0.0));
        write_wav_48k(
            &artifacts().join(format!("speech_{kind}_pause_speech_on.wav")),
            &output,
        )?;
    }
    Ok(())
}
#[test]
fn level_jumps_and_toggles_keep_finite_peaks() -> anyhow::Result<()> {
    let speech = load_speech_48k("speech_2")?;
    let mut capture = VoiceCapture::new(VoiceCaptureConfig::default())?;
    let mut output = Vec::new();
    let mut enabled = true;
    for (index, chunk) in speech
        .iter()
        .copied()
        .cycle()
        .take(12 * SR)
        .collect::<Vec<_>>()
        .as_chunks::<FRAME_SIZE_48K>()
        .0
        .iter()
        .enumerate()
    {
        if index % 50 == 0 {
            capture.set_input_gain(if index % 100 == 0 { 3.0 } else { 1.0 })?;
        }
        if index % 75 == 0 {
            enabled = index % 150 != 0;
            capture.set_voice_enhancement(enabled)?;
        }
        let boost = if !(150..450).contains(&index) {
            0.1
        } else {
            3.0
        };
        let mut frame = chunk.map(|s| (s * boost).clamp(-1.0, 1.0));
        capture.process(&mut frame, 1, index as u32 * 20)?;
        assert!(frame.iter().all(|s| s.is_finite() && s.abs() <= 1.000001));
        if enabled {
            assert!(frame[FRAME_SIZE_48K - 1].abs() <= 0.891252);
            if index % 75 != 0 {
                assert!(frame.iter().all(|s| s.abs() <= 0.891252));
            }
        }
        output.extend_from_slice(&frame);
    }
    write_wav_48k(&artifacts().join("level_jumps_toggle.wav"), &output)?;
    Ok(())
}
