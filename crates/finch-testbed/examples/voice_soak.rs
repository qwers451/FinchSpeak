//! Bounded-memory, real-time production capture soak (no physical device/network claims).
use finch_audio::{VoiceCapture, VoiceCaptureConfig, FRAME_SIZE_48K};
use finch_testbed::audio::load_speech_48k;
use std::{path::PathBuf, time::{Duration, Instant}};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let seconds: u64 = args.get(1).ok_or_else(|| anyhow::anyhow!("seconds required"))?.parse()?;
    anyhow::ensure!(seconds > 0 && seconds <= 7200, "duration outside1..7200s");
    let output = PathBuf::from(args.get(2).ok_or_else(|| anyhow::anyhow!("output required"))?);
    let source = load_speech_48k("speech_1")?;
    let mut capture = VoiceCapture::new(VoiceCaptureConfig::default())?;
    let mut histogram = [0u64; 2001]; // bounded,10us bins up to20ms
    let (mut packets, mut private_frames, mut rejoins, mut late) = (0u64, 0u64, 0u64, 0u64);
    let mut max_us = 0u64;
    let start = Instant::now();
    for index in 0..seconds * 50 {
        if index > 0 && index % 1500 == 0 {
            capture = VoiceCapture::new(VoiceCaptureConfig::default())?;
            rejoins += 1;
        }
        let gate = index % 250 < 200;
        capture.set_transmit_gate(gate);
        let enabled = index % 500 < 400;
        capture.set_voice_enhancement(enabled)?;
        capture.set_input_gain(if index % 1000 < 500 { 1.0 } else { 3.0 })?;
        let mut frame = [0.0; FRAME_SIZE_48K];
        for (j, sample) in frame.iter_mut().enumerate() {
            *sample = source[(index as usize * FRAME_SIZE_48K + j) % source.len()] * if index % 750 < 375 { 0.05 } else { 0.7 };
        }
        let began = Instant::now();
        let result = capture.process(&mut frame, 1, index as u32 * 20)?;
        let us = began.elapsed().as_micros() as u64;
        max_us = max_us.max(us);
        histogram[(us / 10).min(2000) as usize] += 1;
        if !gate { anyhow::ensure!(result.packets.is_empty(), "private audio escaped"); private_frames += 1; }
        anyhow::ensure!(frame.iter().all(|s| s.is_finite() && s.abs() <= 1.000001), "invalid PCM");
        packets += result.packets.len() as u64;
        let deadline = start + Duration::from_millis((index + 1) * 20);
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) { std::thread::sleep(wait); } else { late += 1; }
    }
    let mut count = 0;
    let p95 = histogram.iter().position(|n| { count += n; count >= seconds * 50 * 95 / 100 }).unwrap_or(2000) * 10;
    std::fs::write(output, serde_json::to_vec_pretty(&serde_json::json!({"seconds":seconds,"frames":seconds*50,"packets":packets,"private_frames":private_frames,"rejoins":rejoins,"processing_errors":0,"max_dsp_us":max_us,"p95_dsp_us_upper":p95+10,"late_deadlines":late,"scope":"real-time production capture, no physical devices or network"}))?)?;
    std::thread::sleep(Duration::from_secs(5)); // allow final external private-working-set sample
    Ok(())
}

