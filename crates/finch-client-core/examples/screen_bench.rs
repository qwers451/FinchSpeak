//! Where does a screen-share frame spend its time?  `cargo run --release -p finch-client-core --example screen_bench`
use std::time::Instant;

use finch_client_core::screen::{ScreenPacketizer, ShareConfig};
use finch_protocol::media::MAX_DATAGRAM_SIZE;

fn main() {
    let mut packetizer = ScreenPacketizer::new(ShareConfig::default(), 1, 1);
    let (mut capture, mut push) = (0.0, 0.0);
    let frames = 60;
    for i in 0..frames {
        let t = Instant::now();
        let frame = match finch_screen::capture_monitor(0) {
            Ok(frame) => frame,
            Err(e) => return eprintln!("capture failed: {e}"),
        };
        capture += t.elapsed().as_secs_f64();
        let t = Instant::now();
        if let Err(e) = packetizer.push_frame(&frame, i * 33, MAX_DATAGRAM_SIZE) {
            return eprintln!("encode failed: {e}");
        }
        push += t.elapsed().as_secs_f64();
    }
    let n = f64::from(frames);
    println!("capture {:.1} ms/frame, convert+encode+packetize {:.1} ms/frame", capture / n * 1000.0, push / n * 1000.0);
}
