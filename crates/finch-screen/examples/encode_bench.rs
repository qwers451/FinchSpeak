//! How long does OpenH264 take per frame on this build?
//! `cargo run --release -p finch-screen --example encode_bench [-- <width> <height> <fps>]`
//!
//! Encodes a synthetic screen (text-like blocks that scroll a few rows per frame), so it runs
//! without a display. A build without NASM has no SIMD in OpenH264 and is several times slower.
use std::time::Instant;

use finch_screen::{EncodeOutcome, H264VideoEncoder, VideoEncoderConfig};

/// I420 picture of `w`x`h`: blocky "text" that moves down by `shift` rows.
fn synthetic_i420(w: usize, h: usize, shift: usize, out: &mut Vec<u8>) {
    out.clear();
    out.resize(w * h * 3 / 2, 128);
    for y in 0..h {
        let sy = (y + shift) % h;
        let row = &mut out[y * w..(y + 1) * w];
        for (x, pixel) in row.iter_mut().enumerate() {
            // 8x16 cells, each either "paper" or "ink", from a cheap hash of the cell position.
            let cell = (x / 8).wrapping_mul(2_654_435_761) ^ (sy / 16).wrapping_mul(40_503);
            *pixel = if cell % 5 < 2 && sy % 16 > 3 { 40 } else { 235 };
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1).filter_map(|v| v.parse::<u32>().ok());
    let (w, h, fps) = (args.next().unwrap_or(1920), args.next().unwrap_or(1080), args.next().unwrap_or(30) as u8);
    let bitrate = (u64::from(w) * u64::from(h) * u64::from(fps) / 12).clamp(1_000_000, 10_000_000) as u32;
    let mut encoder = match H264VideoEncoder::with_config(VideoEncoderConfig { width: w, height: h, fps, bitrate_bps: bitrate }) {
        Ok(encoder) => encoder,
        Err(e) => return eprintln!("encoder failed: {e}"),
    };
    let frames = 120usize;
    let mut yuv = Vec::new();
    let (mut total, mut worst, mut bytes) = (0.0f64, 0.0f64, 0usize);
    for i in 0..frames {
        synthetic_i420(w as usize, h as usize, i * 6, &mut yuv);
        let started = Instant::now();
        match encoder.encode_frame(&yuv) {
            Ok(EncodeOutcome::Frame(frame)) => bytes += frame.data.len(),
            Ok(EncodeOutcome::Skipped) => {}
            Err(e) => return eprintln!("encode failed: {e}"),
        }
        let spent = started.elapsed().as_secs_f64();
        // The first frame is a key frame and pays for start-up; it is not part of the average.
        if i > 0 {
            total += spent;
            worst = worst.max(spent);
        }
    }
    let average_ms = total / (frames - 1) as f64 * 1000.0;
    println!("{w}x{h} at {fps} fps: {average_ms:.1} ms/frame (worst {:.1} ms), frame budget {:.1} ms, {:.0} kbit/s",
        worst * 1000.0, 1000.0 / f64::from(fps), bytes as f64 * 8.0 / (frames as f64 / f64::from(fps)) / 1000.0);
}
