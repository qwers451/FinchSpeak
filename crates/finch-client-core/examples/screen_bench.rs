//! Where does a screen-share frame spend its time?
//! `cargo run --release -p finch-client-core --example screen_bench [-- <fps> [<max_height>]]`
//!
//! Prints the cost of each stage for the main monitor: capture, pixel conversion, H.264 encoding.
//! The picture is scrolled by a few rows every frame so the encoder has real work to do.
use std::time::Instant;

use finch_client_core::screen::encoded_size;
use finch_screen::yuv::{packed_to_i420, PixelOrder};
use finch_screen::{EncodeOutcome, H264VideoEncoder, VideoEncoderConfig};

fn main() {
    let mut args = std::env::args().skip(1);
    let fps: u8 = args.next().and_then(|v| v.parse().ok()).unwrap_or(30);
    let max_height: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(u32::MAX);
    let frames = 90u32;

    let first = match finch_screen::capture_monitor(0) {
        Ok(frame) => frame,
        Err(e) => return eprintln!("capture failed: {e}"),
    };
    let (src_w, src_h) = (first.width as usize, first.height as usize);
    let (mut w, mut h) = encoded_size(first.width, first.height);
    if h > max_height {
        w = (u64::from(w) * u64::from(max_height) / u64::from(h)) as u32 & !1;
        h = max_height & !1;
    }
    let bitrate = (u64::from(w) * u64::from(h) * u64::from(fps) / 12).clamp(1_000_000, 10_000_000) as u32;
    let mut encoder = match H264VideoEncoder::with_config(VideoEncoderConfig { width: w, height: h, fps, bitrate_bps: bitrate }) {
        Ok(encoder) => encoder,
        Err(e) => return eprintln!("encoder failed: {e}"),
    };

    let (mut capture, mut scale, mut convert, mut encode) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut worst_encode = 0.0f64;
    let mut bytes = 0usize;
    let (out_w, out_h) = (w as usize, h as usize);
    let mut picture = vec![0u8; out_w * out_h * 4];
    let mut yuv = Vec::new();
    for i in 0..frames {
        let t = Instant::now();
        let frame = match finch_screen::capture_monitor(0) {
            Ok(frame) => frame,
            Err(e) => return eprintln!("capture failed: {e}"),
        };
        capture += t.elapsed().as_secs_f64();

        // Fit to the encoded size, shifted down by a few rows per frame (movement for the encoder).
        let t = Instant::now();
        let shift = (i as usize * 8) % src_h;
        for y in 0..out_h {
            let sy = ((y * src_h / out_h) + shift) % src_h;
            let row = &frame.data[sy * src_w * 4..][..src_w * 4];
            let out = &mut picture[y * out_w * 4..][..out_w * 4];
            if out_w == src_w {
                out.copy_from_slice(row);
            } else {
                for x in 0..out_w {
                    let sx = (x * src_w / out_w) * 4;
                    out[x * 4..x * 4 + 4].copy_from_slice(&row[sx..sx + 4]);
                }
            }
        }
        scale += t.elapsed().as_secs_f64();

        let t = Instant::now();
        packed_to_i420(&picture, out_w, out_h, PixelOrder::Rgba, &mut yuv);
        convert += t.elapsed().as_secs_f64();

        let t = Instant::now();
        match encoder.encode_frame(&yuv) {
            Ok(EncodeOutcome::Frame(encoded)) => bytes += encoded.data.len(),
            Ok(EncodeOutcome::Skipped) => {}
            Err(e) => return eprintln!("encode failed: {e}"),
        }
        let spent = t.elapsed().as_secs_f64();
        encode += spent;
        worst_encode = worst_encode.max(spent);
    }
    let n = f64::from(frames);
    let ms = |total: f64| total / n * 1000.0;
    println!("screen {src_w}x{src_h} -> encoded {w}x{h} at {fps} fps, target {} kbit/s", bitrate / 1000);
    println!("capture      {:6.1} ms/frame", ms(capture));
    println!("fit/copy     {:6.1} ms/frame", ms(scale));
    println!("to I420      {:6.1} ms/frame", ms(convert));
    println!("H.264 encode {:6.1} ms/frame (worst {:.1} ms)", ms(encode), worst_encode * 1000.0);
    println!("encode+convert budget: {:.1} ms of {:.1} ms per frame", ms(scale + convert + encode), 1000.0 / f64::from(fps));
    println!("produced {:.0} kbit/s", bytes as f64 * 8.0 / (n / f64::from(fps)) / 1000.0);
}
