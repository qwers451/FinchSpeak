//! Does loopback capture of the system sound work on this PC?  Play something while it runs.
//! `cargo run --release -p finch-client-core --example screen_audio_probe`
use std::time::{Duration, Instant};

use finch_screen::{ScreenAudioCapturer, ScreenAudioConfig};

fn main() {
    let capturer = match ScreenAudioCapturer::start(ScreenAudioConfig::default()) {
        Ok(capturer) => capturer,
        Err(e) => return eprintln!("loopback capture failed to start: {e}"),
    };
    let (start, mut packets, mut bytes) = (Instant::now(), 0u32, 0usize);
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(packet) = capturer.receiver().recv_timeout(Duration::from_millis(100)) {
            packets += 1;
            bytes += packet.payload.len();
        }
    }
    println!("{packets} packets, {bytes} bytes in 5 s");
}
