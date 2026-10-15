//! Test signals, speech fixtures and WAV helpers.

use std::path::{Path, PathBuf};

use rand::{Rng, SeedableRng};
use rubato::{FftFixedInOut, Resampler};

use crate::metrics::power;

/// Sample rate of the stored speech fixtures.
const FIXTURE_RATE: u32 = 16_000;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(format!("{name}.wav"))
}

/// Load a 16 kHz speech fixture and upsample it to 48 kHz.
///
/// Uses rubato directly: `finch_audio::InputResampler` drops samples for
/// non-48 kHz input (AUD-048) and cannot be trusted to prepare reference signals.
pub fn load_speech_48k(name: &str) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(fixture_path(name))?;
    let spec = reader.spec();
    anyhow::ensure!(
        spec.sample_rate == FIXTURE_RATE && spec.channels == 1 && spec.bits_per_sample == 16,
        "fixture {name} must be 16 kHz mono 16-bit, got {spec:?}"
    );
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / 32768.0))
        .collect::<Result<_, _>>()?;

    let chunk_in = 320; // 20 ms at 16 kHz -> 960 samples at 48 kHz
    let mut resampler = FftFixedInOut::<f32>::new(FIXTURE_RATE as usize, 48_000, chunk_in, 1)?;
    let mut out = Vec::with_capacity(samples.len() * 3);
    for chunk in samples.chunks(chunk_in) {
        let mut input = chunk.to_vec();
        input.resize(chunk_in, 0.0);
        let waves = resampler.process(&[input], None)?;
        if let Some(channel) = waves.first() {
            out.extend_from_slice(channel);
        }
    }
    Ok(out)
}

fn scale_to_dbfs(mut x: Vec<f32>, rms_dbfs: f32) -> Vec<f32> {
    let current = power(&x).max(1e-12);
    let target = 10f64.powf(rms_dbfs as f64 / 10.0);
    let gain = (target / current).sqrt() as f32;
    for s in &mut x {
        *s *= gain;
    }
    x
}

/// Uniform white noise at the given RMS level.
pub fn white_noise(len: usize, rms_dbfs: f32, seed: u64) -> Vec<f32> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let raw = (0..len).map(|_| rng.gen_range(-1.0f32..1.0)).collect();
    scale_to_dbfs(raw, rms_dbfs)
}

/// Pink noise (Paul Kellet's refined filter) at the given RMS level.
pub fn pink_noise(len: usize, rms_dbfs: f32, seed: u64) -> Vec<f32> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut b = [0.0f32; 7];
    let raw = (0..len)
        .map(|_| {
            let white = rng.gen_range(-1.0f32..1.0);
            b[0] = 0.99886 * b[0] + white * 0.055_517_9;
            b[1] = 0.99332 * b[1] + white * 0.075_075_9;
            b[2] = 0.96900 * b[2] + white * 0.153_852;
            b[3] = 0.86650 * b[3] + white * 0.310_485_6;
            b[4] = 0.55000 * b[4] + white * 0.532_952_2;
            b[5] = -0.7616 * b[5] - white * 0.016_898;
            let out = b.iter().sum::<f32>() + white * 0.5362;
            b[6] = white * 0.115_926;
            out
        })
        .collect();
    scale_to_dbfs(raw, rms_dbfs)
}

/// Babble noise: the three speech fixtures summed with random circular offsets.
pub fn babble(len: usize, rms_dbfs: f32, seed: u64) -> anyhow::Result<Vec<f32>> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut out = vec![0.0f32; len];
    for name in ["speech_1", "speech_2", "speech_3"] {
        let speech = load_speech_48k(name)?;
        let offset = rng.gen_range(0..speech.len());
        for (i, s) in out.iter_mut().enumerate() {
            *s += speech[(i + offset) % speech.len()];
        }
    }
    Ok(scale_to_dbfs(out, rms_dbfs))
}

/// Linear sine sweep from `f0` to `f1` Hz with amplitude 0.5.
pub fn chirp(len: usize, f0: f32, f1: f32) -> Vec<f32> {
    let duration = len as f32 / 48_000.0;
    (0..len)
        .map(|i| {
            let t = i as f32 / 48_000.0;
            let phase =
                2.0 * std::f32::consts::PI * (f0 * t + (f1 - f0) * t * t / (2.0 * duration));
            0.5 * phase.sin()
        })
        .collect()
}

/// Gain applied to `noise` so that `speech` over the scaled noise has `snr_db`.
pub fn noise_scale_for(speech: &[f32], noise: &[f32], snr_db: f32) -> f32 {
    let p_speech = power(speech);
    let p_noise = power(&noise[..speech.len().min(noise.len())]).max(1e-12);
    ((p_speech / p_noise) / 10f64.powf(snr_db as f64 / 10.0)).sqrt() as f32
}

/// `speech + noise·g` with `g` chosen for the requested SNR; length of `speech`.
pub fn mix_at_snr(speech: &[f32], noise: &[f32], snr_db: f32) -> Vec<f32> {
    let g = noise_scale_for(speech, noise, snr_db);
    speech
        .iter()
        .zip(noise.iter().chain(std::iter::repeat(&0.0)))
        .map(|(&s, &n)| s + n * g)
        .collect()
}

/// Write a 48 kHz mono float WAV (for inspecting failures).
pub fn write_wav_48k(path: &Path, samples: &[f32]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}
