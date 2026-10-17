//! Voice quality metrics: end-to-end latency, noise suppression, echo cancellation, VAD.
//!
//! Thresholds come from the spec (docs/superpowers/specs/2026-10-15-audit-and-testbed-design.md)
//! and must not be lowered without the owner's approval. Measured values go to BASELINE.md.

use finch_audio::{PREROLL_FRAMES, VoiceCapture, VoiceCaptureConfig, FRAME_SIZE_48K, VadMode, DEFAULT_VAD_THRESHOLD_DB};
use finch_testbed::audio::{babble, chirp, load_speech_48k, mix_at_snr, pink_noise, white_noise, write_wav_48k};
use finch_testbed::client::TestClient;
use finch_testbed::metrics::{delay_samples, erle_db, power, rms_dbfs, segmental_snr_db, snr_db};
use finch_testbed::server::TestServer;
use finch_testbed::voice::{run_voice_link, run_voice_link_with};

const SR: usize = 48_000;

/// Audio sent ahead of the detected speech start (see `PREROLL_FRAMES`).
const PREROLL_MS: u32 = PREROLL_FRAMES as u32 * 20;

fn debug_wav(name: &str, samples: &[f32]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/testbed-out")
        .join(format!("{name}.wav"));
    let _ = write_wav_48k(&path, samples);
}

/// Run `input` through a fresh capture pipeline frame by frame and return the processed signal.
fn process_capture(input: &[f32], far_end: Option<&[f32]>) -> Vec<f32> {
    process_capture_with(input, far_end, true)
}

// Isolated AEC/NS controls exclude AGC, whose changing gain biases separate-pause SNR.
// Full default-on A/B uses the fixed common voiced windows in voice_enhancement.rs.
fn process_capture_with(input: &[f32], far_end: Option<&[f32]>, voice_enhancement: bool) -> Vec<f32> {
    let mut capture = VoiceCapture::new(VoiceCaptureConfig { voice_enhancement, ..Default::default() }).expect("capture pipeline");
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
            capture.push_playback_reference(&reference).unwrap();
        }
        capture.process(&mut frame, 1, (i * 20) as u32).unwrap();
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

    // The chirp is a transport probe, not speech: loudness-threshold VAD.
    let run = run_voice_link_with(&a, &mut b, &input, 10, VadMode::Energy, DEFAULT_VAD_THRESHOLD_DB).await?;
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
    let output = process_capture_with(&input, None, false);
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

    let output = process_capture_with(&capture_in, Some(&far), false);
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
    let full = process_capture(&capture_in, Some(&far));
    let full_erle = erle_db(&capture_in[2 * SR..4 * SR], &full[2 * SR..4 * SR]);
    let full_near = rms_dbfs(&full[4 * SR..6 * SR]);
    eprintln!("BASELINE aec_default_on: erle={full_erle:.1} dB, double_talk={full_near:.1} dBFS, near={near_level:.1}");
    assert!(full_erle >= 20.0, "default-on ERLE {full_erle:.1} <20dB");
    assert!(full_near >= near_level - 6.0, "default-on near speech lost more than6dB");
    debug_wav("aec_default_on", &full);
}

#[tokio::test]
async fn silence_and_noise_do_not_light_speaking() -> anyhow::Result<()> {
    let (_server, a, mut b) = linked_pair().await?;

    let mut input = vec![0.0f32; 3 * SR];
    input.extend(pink_noise(3 * SR, -35.0, 7));
    let speech_start = input.len();
    input.extend(load_speech_48k("speech_1")?);

    let run = run_voice_link(&a, &mut b, &input, 10).await?;
    let lit: Vec<_> = run
        .speaking_events
        .iter()
        .filter(|(frame, _, speaking)| *speaking && *frame < speech_start / FRAME_SIZE_48K)
        .collect();
    let non_silent = run
        .received_headers
        .iter()
        // The pre-roll sent when speech starts reaches back PREROLL_FRAMES before it.
        .filter(|h| !h.is_silence() && h.timestamp + PREROLL_MS < (speech_start / 48) as u32)
        .count();
    let speech_lit = run.speaking_events.iter().any(|(frame, _, speaking)| {
        *speaking && *frame >= speech_start / FRAME_SIZE_48K
    });
    let speech_packets = run.received_headers.iter().filter(|h| {
        !h.is_silence() && h.timestamp >= (speech_start / 48) as u32
    }).count();
    eprintln!(
        "BASELINE vad_noise: speaking_events_on = {}, non_silent_packets = {non_silent}, speech_lit = {speech_lit}, speech_packets = {speech_packets}",
        lit.len()
    );
    assert!(lit.is_empty(), "speaking indicator lit on noise: {lit:?}");
    assert_eq!(non_silent, 0, "non-silent packets sent for noise only");
    assert!(
        speech_lit,
        "real speech must light the receiver's speaking indicator"
    );
    assert!(
        speech_packets > 0,
        "real speech must reach the receiver"
    );
    Ok(())
}

/// Timestamps (ms) of every packet VoiceCapture would send for `input`, plus the VAD verdicts.
fn transmitted(input: &[f32], mode: VadMode) -> Vec<u32> {
    let mut capture = VoiceCapture::new(VoiceCaptureConfig { vad_mode: mode, ..VoiceCaptureConfig::default() })
        .expect("capture pipeline");
    let mut timestamps = Vec::new();
    let (chunks, _) = input.as_chunks::<FRAME_SIZE_48K>();
    for (i, chunk) in chunks.iter().enumerate() {
        let mut frame = *chunk;
        let out = capture.process(&mut frame, 1, (i * 20) as u32).unwrap();
        for packet in out.packets {
            let header = finch_protocol::media::MediaHeader::from_slice(&packet[..20]).expect("header");
            timestamps.push(header.timestamp);
        }
    }
    timestamps
}

/// Mic hiss (white noise at -45 dBFS) is not transmitted. White noise at -35 dBFS, a far louder
/// hiss than any real microphone floor, still fools the classifier on ~3% of the frames
/// (measured, BASELINE.md); pink noise (fans, rooms) is covered at -35 dBFS above.
#[test]
fn white_noise_hiss_is_not_transmitted() {
    let noise = white_noise(3 * SR, -45.0, 7);
    assert!(transmitted(&noise, VadMode::Speech).is_empty());
}

/// The classifier opens 1-3 frames after a word starts; the pre-roll must make up for it. Both
/// modes send a pre-roll, so the loudness detector's own detection time is its first packet plus
/// `PREROLL_MS`; the speech classifier's stream must start no later than that.
#[test]
fn speech_onset_is_not_clipped() {
    for name in ["speech_1", "speech_2", "speech_3"] {
        let mut input = vec![0.0f32; SR];
        input.extend(load_speech_48k(name).expect("fixture"));

        let first_speech = transmitted(&input, VadMode::Speech).into_iter().next().expect("speech is sent");
        let first_loud = transmitted(&input, VadMode::Energy).into_iter().next().expect("speech is loud");
        let loud_detected = first_loud + PREROLL_MS;
        eprintln!(
            "BASELINE vad_onset {name}: speech-mode stream starts at {first_speech} ms, loudness threshold detects at {loud_detected} ms"
        );
        assert!(
            first_speech <= loud_detected,
            "{name}: the start of the utterance is clipped ({first_speech} ms > {loud_detected} ms)"
        );
    }
}
