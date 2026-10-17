//! Thin ownership/buffer adapters for the original C++ WebRTC APM.
//! No adaptive gain or compression algorithm is implemented here.
use crate::{capture::MAX_INPUT_GAIN, resampler::FRAME_SIZE_48K};
use std::{cell::Cell, ffi::c_void, marker::PhantomData, ptr::NonNull};
const BLOCK: usize = FRAME_SIZE_48K / 2;
const CEILING: f32 = 0.891_250_9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("WebRTC {operation} failed (status {status})")]
pub struct ProcessingError {
    pub operation: &'static str,
    pub status: i32,
}

#[link(name = "finch_webrtc")]
unsafe extern "C" {
    fn finch_webrtc_create(stage: i32, output: *mut *mut c_void) -> i32;
    fn finch_webrtc_destroy(handle: *mut c_void);
    fn finch_webrtc_capture(
        handle: *mut c_void,
        input: *const f32,
        output: *mut f32,
        length: u32,
    ) -> i32;
    fn finch_webrtc_render(handle: *mut c_void, input: *const f32, length: u32) -> i32;
    fn finch_webrtc_reset(handle: *mut c_void) -> i32;
    fn finch_webrtc_set_delay(handle: *mut c_void, milliseconds: u32) -> i32;
    fn finch_webrtc_set_fixed_gain(handle: *mut c_void, decibels: f32) -> i32;
}
fn checked(operation: &'static str, status: i32) -> Result<(), ProcessingError> {
    if status == 0 {
        Ok(())
    } else {
        Err(ProcessingError { operation, status })
    }
}
fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

struct NativeHandle {
    pointer: NonNull<c_void>,
    _not_sync: PhantomData<Cell<()>>,
    #[cfg(test)]
    capture_calls: usize,
    #[cfg(test)]
    render_calls: usize,
    #[cfg(test)]
    resets: usize,
    #[cfg(test)]
    fail_after_capture: Option<usize>,
    #[cfg(test)]
    fail_render: bool,
}
// SAFETY: exclusive ownership may move to another worker; no shared access is exposed.
// Cell marker keeps the handle !Sync. Every native call requires &mut self.
unsafe impl Send for NativeHandle {}
impl NativeHandle {
    fn new(stage: i32) -> Result<Self, ProcessingError> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: out-pointer is valid; the C ABI returns an exclusively owned handle or null.
        checked("initialize", unsafe {
            finch_webrtc_create(stage, &mut raw)
        })?;
        Ok(Self {
            pointer: NonNull::new(raw).ok_or(ProcessingError {
                operation: "initialize",
                status: -1002,
            })?,
            _not_sync: PhantomData,
            #[cfg(test)]
            capture_calls: 0,
            #[cfg(test)]
            render_calls: 0,
            #[cfg(test)]
            resets: 0,
            #[cfg(test)]
            fail_after_capture: None,
            #[cfg(test)]
            fail_render: false,
        })
    }
    fn process(&mut self, frame: &mut [f32; FRAME_SIZE_48K]) -> Result<(), ProcessingError> {
        let input = frame.map(sanitize);
        let mut output = [0.0; FRAME_SIZE_48K];
        for (source, destination) in input
            .as_chunks::<BLOCK>()
            .0
            .iter()
            .zip(output.as_chunks_mut::<BLOCK>().0.iter_mut())
        {
            #[cfg(test)]
            {
                if self.fail_after_capture == Some(0) {
                    return checked("capture", -1099);
                }
                if let Some(remaining) = &mut self.fail_after_capture {
                    *remaining -= 1;
                }
                self.capture_calls += 1;
            }
            // SAFETY: both slices contain exactly BLOCK floats; no aliases; handle is exclusively owned.
            checked("capture", unsafe {
                finch_webrtc_capture(
                    self.pointer.as_ptr(),
                    source.as_ptr(),
                    destination.as_mut_ptr(),
                    BLOCK as u32,
                )
            })?;
        }
        if !output.iter().all(|s| s.is_finite()) {
            return checked("non-finite output", -1003);
        }
        *frame = output; // Publish only after both blocks succeed.
        Ok(())
    }
    fn render(&mut self, frame: &[f32; FRAME_SIZE_48K]) -> Result<(), ProcessingError> {
        let input = frame.map(sanitize);
        for source in input.as_chunks::<BLOCK>().0.iter() {
            #[cfg(test)]
            {
                if self.fail_render {
                    return checked("render", -1099);
                }
                self.render_calls += 1;
            }
            // SAFETY: source is BLOCK valid floats for the duration of this synchronous call.
            checked("render", unsafe {
                finch_webrtc_render(self.pointer.as_ptr(), source.as_ptr(), BLOCK as u32)
            })?;
        }
        Ok(())
    }
    fn reset(&mut self) -> Result<(), ProcessingError> {
        #[cfg(test)]
        {
            self.resets += 1;
        }
        // SAFETY: valid exclusively owned handle. No buffer survives this call.
        checked("reset", unsafe {
            finch_webrtc_reset(self.pointer.as_ptr())
        })
    }
    fn set_gain(&mut self, db: f32) -> Result<(), ProcessingError> {
        // SAFETY: valid exclusive handle, db is bounded/finite by the caller and checked in C++.
        checked("fixed gain", unsafe {
            finch_webrtc_set_fixed_gain(self.pointer.as_ptr(), db)
        })
    }
}
impl Drop for NativeHandle {
    fn drop(&mut self) {
        // SAFETY: created once by the C ABI; no clone/shared ownership; destroyed exactly once here.
        unsafe { finch_webrtc_destroy(self.pointer.as_ptr()) };
    }
}

pub struct WebRtcAec {
    native: NativeHandle,
}
impl WebRtcAec {
    pub fn new() -> Result<Self, ProcessingError> {
        Ok(Self {
            native: NativeHandle::new(0)?,
        })
    }
    pub fn push_render(&mut self, frame: &[f32; FRAME_SIZE_48K]) -> Result<(), ProcessingError> {
        self.native.render(frame)
    }
    pub fn process(&mut self, frame: &mut [f32; FRAME_SIZE_48K]) -> Result<(), ProcessingError> {
        self.native.process(frame)
    }
    pub fn reset(&mut self) -> Result<(), ProcessingError> {
        self.native.reset()
    }
    pub fn set_delay_ms(&mut self, delay: u32) -> Result<(), ProcessingError> {
        // SAFETY: exclusive valid handle; the C ABI accepts delays 0..=500.
        checked("delay", unsafe {
            finch_webrtc_set_delay(self.native.pointer.as_ptr(), delay.min(500))
        })
    }
}

pub struct WebRtcGain {
    native: NativeHandle,
    input_gain: f32,
    enabled: bool,
    wet: f32,
}
impl WebRtcGain {
    #[cfg(test)]
    pub(crate) fn fail_next_capture_for_test(&mut self) {
        self.native.fail_after_capture = Some(0);
    }
    pub fn new() -> Result<Self, ProcessingError> {
        Ok(Self {
            native: NativeHandle::new(1)?,
            input_gain: 1.0,
            enabled: true,
            wet: 1.0,
        })
    }
    pub fn set_input_gain(&mut self, gain: f32) -> Result<(), ProcessingError> {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, MAX_INPUT_GAIN)
        } else {
            1.0
        };
        if gain == self.input_gain {
            return Ok(());
        }
        // Keep the latest requested trim; enqueue at most one native update per frame.
        // In bypass mode the native processor does not drain runtime-setting messages.
        self.input_gain = gain;
        Ok(())
    }
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), ProcessingError> {
        if enabled == self.enabled {
            return Ok(());
        }
        if enabled {
            self.native.reset()?;
        }
        self.enabled = enabled;
        Ok(())
    }
    pub fn reset(&mut self) -> Result<(), ProcessingError> {
        self.native.reset()?;
        self.wet = if self.enabled { 1.0 } else { 0.0 };
        Ok(())
    }
    pub fn process(&mut self, frame: &mut [f32; FRAME_SIZE_48K]) -> Result<(), ProcessingError> {
        let source = frame.map(sanitize);
        let dry = source.map(|s| (s * self.input_gain).clamp(-1.0, 1.0));
        if !self.enabled && self.wet == 0.0 {
            *frame = dry;
            return Ok(());
        }
        let mut processed = source.map(|s| s * self.input_gain.min(1.0));
        self.native
            .set_gain(20.0 * self.input_gain.max(1.0).log10())?;
        self.native.process(&mut processed)?;
        let target = if self.enabled { 1.0 } else { 0.0 };
        let start = self.wet;
        for i in 0..FRAME_SIZE_48K {
            let wet = start + (target - start) * ((i + 1) as f32 / FRAME_SIZE_48K as f32);
            // Constant headroom after upstream limiter, not a second compression algorithm.
            frame[i] = dry[i] * (1.0 - wet) + processed[i] * CEILING * wet;
        }
        self.wet = target;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tone(index: usize, amplitude: f32) -> [f32; FRAME_SIZE_48K] {
        std::array::from_fn(|i| {
            amplitude
                * (2.0 * std::f32::consts::PI * 200.0 * (index * FRAME_SIZE_48K + i) as f32
                    / 48_000.0)
                    .sin()
        })
    }
    #[test]
    fn native_adapter_preserves_frame_size_and_block_order() {
        let mut aec = WebRtcAec::new().unwrap();
        let frame = tone(0, 0.1);
        aec.push_render(&frame).unwrap();
        let mut output = frame;
        aec.process(&mut output).unwrap();
        assert_eq!(aec.native.capture_calls, 2);
        assert_eq!(aec.native.render_calls, 2);
        assert!(output.iter().all(|s| s.is_finite()));
    }
    #[test]
    fn native_failure_does_not_publish_a_partial_frame() {
        let mut aec = WebRtcAec::new().unwrap();
        aec.native.fail_after_capture = Some(1);
        let mut frame = tone(0, 0.1);
        let original = frame;
        assert!(aec.process(&mut frame).is_err());
        assert_eq!(frame, original);
        aec.native.fail_render = true;
        assert!(aec.push_render(&original).is_err());
        assert!(NativeHandle::new(99).is_err());
    }
    #[test]
    fn gain_is_safe_for_invalid_samples_and_manual_trim() {
        let mut gain = WebRtcGain::new().unwrap();
        for trim in [0.0, 0.5, 1.0, 3.0, 8.0, f32::NAN] {
            gain.set_input_gain(trim).unwrap();
            for index in 0..10 {
                let mut frame = tone(index, 0.9);
                frame[0] = f32::NAN;
                frame[1] = f32::INFINITY;
                frame[2] = f32::MAX;
                gain.process(&mut frame).unwrap();
                assert!(
                    frame
                        .iter()
                        .all(|s| s.is_finite() && s.abs() <= CEILING + 1e-6)
                );
                if trim == 0.0 {
                    assert!(frame.iter().all(|s| *s == 0.0));
                }
            }
        }
    }
    #[test]
    fn bypass_and_toggle_are_smooth_and_setters_are_idempotent() {
        let mut gain = WebRtcGain::new().unwrap();
        gain.set_enabled(false).unwrap();
        gain.reset().unwrap();
        let mut dry = [0.02; FRAME_SIZE_48K];
        gain.process(&mut dry).unwrap();
        assert_eq!(dry, [0.02; FRAME_SIZE_48K]);
        gain.set_enabled(true).unwrap();
        let resets = gain.native.resets;
        gain.set_enabled(true).unwrap();
        gain.set_input_gain(1.0).unwrap();
        gain.set_input_gain(1.0).unwrap();
        assert_eq!(gain.native.resets, resets);
        let mut frame = [0.02; FRAME_SIZE_48K];
        gain.process(&mut frame).unwrap();
        assert!((frame[0] - 0.02).abs() < 0.001);
        assert!(frame.windows(2).all(|w| (w[0] - w[1]).abs() < 0.001));
    }
    #[test]
    fn reset_matches_fresh_native_state() {
        let mut used = WebRtcGain::new().unwrap();
        for i in 0..100 {
            used.process(&mut tone(i, 0.01)).unwrap();
        }
        used.reset().unwrap();
        let mut fresh = WebRtcGain::new().unwrap();
        let mut a = tone(0, 0.01);
        let mut b = a;
        used.process(&mut a).unwrap();
        fresh.process(&mut b).unwrap();
        assert_eq!(a, b);
    }
    #[test]
    fn native_repeated_toggle_and_destroy() {
        for i in 0..200 {
            let mut gain = WebRtcGain::new().unwrap();
            gain.set_enabled(i % 2 == 0).unwrap();
            gain.set_input_gain(3.0).unwrap();
            gain.process(&mut tone(i, 0.1)).unwrap();
            gain.reset().unwrap();
        }
    }
    struct CountingAllocator;
    thread_local! {
        static TRACK_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
        static ALLOCATION_COUNT: Cell<usize> = const { Cell::new(0) };
    }
    #[global_allocator]
    static TEST_ALLOCATOR: CountingAllocator = CountingAllocator;
    // SAFETY: every operation delegates unchanged to System; thread-local counters own no heap.
    unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
            let _ = TRACK_ALLOCATIONS.try_with(|track| {
                if track.get() {
                    ALLOCATION_COUNT.with(|count| count.set(count.get() + 1));
                }
            });
            // SAFETY: caller supplies a valid allocation layout.
            unsafe { std::alloc::System.alloc(layout) }
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
            // SAFETY: pointer and layout are passed unchanged to their original allocator.
            unsafe { std::alloc::System.dealloc(pointer, layout) }
        }
    }
    #[test]
    fn sixty_seconds_of_adapter_processing_allocates_nothing() {
        let mut gain = WebRtcGain::new().unwrap();
        for i in 0..100 {
            gain.process(&mut tone(i, 0.03)).unwrap();
        }
        let started = std::time::Instant::now();
        ALLOCATION_COUNT.with(|count| count.set(0));
        TRACK_ALLOCATIONS.with(|track| track.set(true));
        for i in 0..3000 {
            gain.process(&mut tone(i, 0.03)).unwrap();
        }
        TRACK_ALLOCATIONS.with(|track| track.set(false));
        assert_eq!(ALLOCATION_COUNT.with(|count| count.get()), 0);
        eprintln!(
            "NATIVE_DSP: 60 s processed in {:?}; zero Rust adapter allocations",
            started.elapsed()
        );
    }

    #[test]
    fn rapid_trim_changes_apply_the_latest_value_without_filling_native_queue() {
        let mut rapid = WebRtcGain::new().unwrap();
        let mut reference = WebRtcGain::new().unwrap();
        for index in 0..1000 {
            rapid
                .set_input_gain(if index % 2 == 0 { 1.0 } else { 3.0 })
                .unwrap();
        }
        reference.set_input_gain(3.0).unwrap();
        for index in 0..5 {
            let mut actual = tone(index, 0.01);
            let mut expected = actual;
            rapid.process(&mut actual).unwrap();
            reference.process(&mut expected).unwrap();
            if index == 4 {
                assert_eq!(
                    actual, expected,
                    "obsolete queued gains replaced the latest trim"
                );
            }
        }
    }
}
