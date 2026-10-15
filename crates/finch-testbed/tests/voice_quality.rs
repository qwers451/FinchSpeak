//! Voice quality metrics: end-to-end latency, noise suppression, echo cancellation, VAD.
//!
//! Thresholds come from the spec (docs/superpowers/specs/2026-10-15-audit-and-testbed-design.md)
//! and must not be lowered without the owner's approval. Measured values go to BASELINE.md.

use finch_audio::{VoiceCapture, VoiceCaptureConfig, FRAME_SIZE_48K};
use finch_testbed::audio::{babble, chirp, load_speech_48k, mix_at_snr, pink_noise, white_noise, write_wav_48k};
use finch_testbed::client::TestClient;
use finch_testbed::metrics::{delay_samples, erle_db, power, rms_dbfs, segmental_snr_db, snr_db};
use finch_testbed::server::TestServer;
use finch_testbed::voice::run_voice_link;

const SR: usize = 48_000;

fn debug_wav(name: &str, samples: &[f32]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/testbed-out")
        .join(format!("{name}.wav"));
    let _ = write_wav_48k(&path, samples);
}

/// Run `input` through a fresh capture pipeline frame by frame and return the processed signal.
fn process_capture(input: &[f32], far_end: Option<&[f32]>) -> Vec<f32> {
    let mut capture = VoiceCapture::new(VoiceCaptureConfig::default()).expect("capture pipeline");
    let mut out = Vec::with_capacity(input.len());
    for (i, chunk) in input.chunks(FRAME_SIZE_48K).enumerate() {
        let mut frame = [0.0f32; FRAME_SIZE_48K];
        frame[..chunk.len()].copy_from_slice(chunk);
        if let Some(far) = far_end {
            let mut reference = [0.0f32; FRAME_SIZE_48K];
            let start = i * FRAME_SIZE_48K;
            if start < far.len() {
                let end = (start + FRAME_SIZE_48K).min(far.len());
                reference[..end - start].copy_from_slice(&far[start..end]);
            }
            capture.push_playback_reference(&reference);
        }
        capture.process(&mut frame, 1, (i * 20) as u32);
        out.extend_from_slice(&frame[..chunk.len()]);
    }
    out
}

async fn linked_pair() -> anyhow::Result<(TestServer, TestClient, TestClient)> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    a.join_voice().await?;
    b.join_voice().await?;
    b.drain_datagrams();
    Ok((server, a, b))
}

#[tokio::test]
async fn end_to_end_latency_under_120ms() -> anyhow::Result<()> {
    let (_server, a, mut b) = linked_pair().await?;

    let mut input = vec![0.0f32; SR / 5];
    let probe = chirp(SR / 10, 300.0, 3_000.0);
    input.extend_from_slice(&probe);
    input.extend(std::iter::repeat_n(0.0, SR));

    let run = run_voice_link(&a, &mut b, &input, 10).await?;
    debug_wav("latency_in", &input);
    debug_wav("latency_out", &run.output);

    let probe_end = SR / 5 + SR / 10;
    assert!(!run.received_headers.is_empty(), "receiver got no audio");
    let latency = delay_samples(&input[..probe_end], &run.output[..probe_end + SR / 2], SR / 4)
        .ok_or_else(|| anyhow::anyhow!("probe not found in receiver output"))?;
    let latency_ms = latency as f32 * 1000.0 / SR as f32;
    eprintln!("BASELINE end_to_end_latency_ms = {latency_ms:.1}");
    assert!(latency_ms <= 120.0, "latency {latency_ms:.1} ms > 120 ms");
    Ok(())
}

/// `enforce`: apply the spec thresholds (stationary noise only, owner decision on AUD-049).
fn check_noise_suppression(kind: &str, noise: Vec<f32>, enforce: bool) {
    let speech = load_speech_48k("speech_1").expect("fixture");
    let lead = SR;
    let noisy_speech = mix_at_snr(&speech, &noise[lead..lead + speech.len()], 5.0);
    let g = finch_testbed::audio::noise_scale_for(&speech, &noise[lead..lead + speech.len()], 5.0);

    let mut input: Vec<f32> = noise[..lead].iter().map(|s| s * g).collect();
    input.extend_from_slice(&noisy_speech);
    let output = process_capture(&input, None);
    debug_wav(&format!("ns_{kind}_in"), &input);
    debug_wav(&format!("ns_{kind}_out"), &output);

    let noise_seg = lead / 2..lead;
    let speech_seg = lead..input.len();
    let snr_in = snr_db(&input[speech_seg.clone()], &input[noise_seg.clone()]);
    let snr_out = snr_db(&output[speech_seg.clone()], &output[noise_seg.clone()]);
    let seg_in = segmental_snr_db(&input[speech_seg.clone()], &speech, power(&input[noise_seg.clone()]));
    let seg_out = segmental_snr_db(&output[speech_seg], &speech, power(&output[noise_seg]));
    eprintln!(
        "BASELINE ns_{kind}: snr_in = {snr_in:.1} dB, snr_out = {snr_out:.1} dB, improvement = {:.1} dB, segsnr_in = {seg_in:.1}, segsnr_out = {seg_out:.1}",
        snr_out - snr_in
    );
    if !enforce {
        return;
    }
    assert!(snr_out - snr_in >= 10.0, "{kind}: SNR improvement {:.1} dB < 10 dB", snr_out - snr_in);
    assert!(seg_out >= seg_in, "{kind}: segmental SNR fell from {seg_in:.1} to {seg_out:.1}");
}

const NS_LEN: usize = SR + 6 * SR;

#[test]
fn noise_suppression_improves_snr_white() {
    check_noise_suppression("white", white_noise(NS_LEN, -30.0, 42), true);
}

#[test]
fn noise_suppression_improves_snr_pink() {
    check_noise_suppression("pink", pink_noise(NS_LEN, -30.0, 42), true);
}

/// Measured only: RNNoise does not suppress competing speech, and the owner limited the
/// SNR threshold to stationary noise (AUD-049). The value goes to BASELINE.md.
#[test]
fn noise_suppression_babble_measured() {
    check_noise_suppression("babble", babble(NS_LEN, -30.0, 42).expect("fixtures"), false);
}

#[test]
#[ignore = "AUD-021"]
fn echo_cancelled_after_convergence() {
    let far_once = load_speech_48k("speech_2").expect("fixture");
    let far: Vec<f32> = far_once.iter().cycle().take(6 * SR).copied().collect();
    let near = load_speech_48k("speech_3").expect("fixture");

    let delay = 1_920; // 40 ms
    let mut capture_in = vec![0.0f32; 6 * SR];
    for i in delay..capture_in.len() {
        capture_in[i] = 0.5 * far[i - delay];
    }
    for i in 0..2 * SR {
        capture_in[4 * SR + i] += near[i];
    }

    let output = process_capture(&capture_in, Some(&far));
    debug_wav("aec_in", &capture_in);
    debug_wav("aec_out", &output);

    let erle = erle_db(&capture_in[2 * SR..4 * SR], &output[2 * SR..4 * SR]);
    let near_level = rms_dbfs(&near[..2 * SR]);
    let double_talk_level = rms_dbfs(&output[4 * SR..6 * SR]);
    eprintln!(
        "BASELINE aec: erle = {erle:.1} dB, near = {near_level:.1} dBFS, double_talk_out = {double_talk_level:.1} dBFS"
    );
    assert!(erle >= 20.0, "ERLE {erle:.1} dB < 20 dB");
    assert!(
        double_talk_level >= near_level - 6.0,
        "near-end speech attenuated to {double_talk_level:.1} dBFS (near {near_level:.1})"
    );
}

#[tokio::test]
#[ignore = "AUD-050"]
async fn silence_and_noise_do_not_light_speaking() -> anyhow::Result<()> {
    let (_server, a, mut b) = linked_pair().await?;

    let mut input = vec![0.0f32; 3 * SR];
    input.extend(pink_noise(3 * SR, -35.0, 7));

    let run = run_voice_link(&a, &mut b, &input, 10).await?;
    let lit: Vec<_> = run.speaking_events.iter().filter(|(_, _, speaking)| *speaking).collect();
    let non_silent = run.received_headers.iter().filter(|h| !h.is_silence()).count();
    eprintln!(
        "BASELINE vad_noise: speaking_events_on = {}, non_silent_packets = {non_silent}",
        lit.len()
    );
    assert!(lit.is_empty(), "speaking indicator lit on noise: {lit:?}");
    assert_eq!(non_silent, 0, "non-silent packets sent for noise only");
    Ok(())
}
