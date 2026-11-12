//! Measures Windows Graphics Capture delivery on the primary monitor for 5 seconds.
//!
//! A still screen legitimately yields few frames: WGC only delivers a frame when
//! the content changes. A background thread prints a line every 10 ms so the
//! console repaints and the screen changes during the run.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use finch_screen::{list_monitors, WgcCapture, WgcTarget};

    const RUN: Duration = Duration::from_secs(5);

    // The share frame rate to capture at (first argument, default 60).
    let fps: u8 = match std::env::args().nth(1) {
        Some(arg) => arg.parse().map_err(|e| format!("FPS must be a number from 1 to 255: {e}"))?,
        None => 60,
    };

    let monitors = list_monitors()?;
    let primary = monitors
        .iter()
        .find(|m| m.is_primary)
        .ok_or("no primary monitor found")?;
    println!(
        "Primary monitor: {} ({}x{}), id {}",
        primary.name, primary.width, primary.height, primary.id
    );

    let capture = WgcCapture::start(WgcTarget::Monitor(primary.id), fps)?;
    println!("Capture limit: {fps} FPS");
    println!("WGC session started. Running for {} s.", RUN.as_secs());
    println!("Note: a still screen legitimately yields few frames; WGC only delivers on change.");

    let stop_printer = Arc::new(AtomicBool::new(false));
    let printer = {
        let stop = stop_printer.clone();
        thread::spawn(move || {
            let mut tick: u64 = 0;
            while !stop.load(Ordering::SeqCst) {
                tick += 1;
                println!("repaint tick {tick}");
                thread::sleep(Duration::from_millis(10));
            }
        })
    };

    let started = Instant::now();
    let mut frames: u64 = 0;
    let mut size = (0u32, 0u32);
    let mut last_arrival: Option<Instant> = None;
    let mut gap_total = Duration::ZERO;
    let mut gap_count: u32 = 0;

    while started.elapsed() < RUN {
        if let Some(frame) = capture.take_frame(Duration::from_millis(100)) {
            frames += 1;
            size = (frame.width, frame.height);
            let now = Instant::now();
            if let Some(prev) = last_arrival {
                gap_total += now.duration_since(prev);
                gap_count += 1;
            }
            last_arrival = Some(now);
        }
    }
    let elapsed = started.elapsed().as_secs_f64();

    stop_printer.store(true, Ordering::SeqCst);
    let _ = printer.join();
    drop(capture);

    println!();
    println!("Frame size: {}x{}", size.0, size.1);
    println!("Frames received: {frames} in {elapsed:.2} s");
    println!("Frames per second: {:.2}", frames as f64 / elapsed);
    if gap_count > 0 {
        let avg_ms = gap_total.as_secs_f64() * 1000.0 / f64::from(gap_count);
        println!("Average time between frames: {avg_ms:.2} ms");
    } else {
        println!("Average time between frames: n/a (fewer than two frames)");
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    println!("Windows only");
}
