//! Windows Graphics Capture (WGC) sessions.
//!
//! A [`WgcCapture`] keeps a capture session open on a background thread. Each
//! frame the OS delivers is copied (without row padding) into a single slot
//! that keeps only the newest frame, so a slow consumer drops frames instead of
//! queueing them.

use std::error::Error as StdError;
use std::ffi::c_void;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use bytes::Bytes;
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

use crate::capture::{PixelFormat, RawFrame, ScreenCaptureError};

/// What to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgcTarget {
    /// Monitor id as returned by `capture::list_monitors()` (`MonitorInfo::id`).
    Monitor(u32),
    /// Window id as returned by `capture::list_windows()` (`WindowInfo::id`, the HWND).
    Window(u32),
}

type HandlerError = Box<dyn StdError + Send + Sync>;
type Control = CaptureControl<FrameHandler, HandlerError>;

/// State shared between the capture thread and the consumer.
struct Shared {
    slot: Mutex<Option<RawFrame>>,
    ready: Condvar,
    closed: AtomicBool,
}

impl Shared {
    fn new() -> Self {
        Self {
            slot: Mutex::new(None),
            ready: Condvar::new(),
            closed: AtomicBool::new(false),
        }
    }

    fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.ready.notify_all();
    }
}

/// Locks the slot, marking the session closed if the lock is poisoned.
fn lock_slot(shared: &Shared) -> Option<MutexGuard<'_, Option<RawFrame>>> {
    match shared.slot.lock() {
        Ok(guard) => Some(guard),
        Err(_) => {
            tracing::warn!("WGC frame slot lock is poisoned; closing capture session");
            shared.mark_closed();
            None
        }
    }
}

/// Handler run by `windows-capture` on its capture thread.
struct FrameHandler {
    shared: Arc<Shared>,
}

impl GraphicsCaptureApiHandler for FrameHandler {
    type Flags = Arc<Shared>;
    type Error = HandlerError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { shared: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        let expected = (width as usize) * (height as usize) * 4;

        let mut buffer = frame.buffer()?;
        let pixels = buffer.as_nopadding_buffer()?;
        if pixels.len() != expected {
            tracing::warn!(
                width,
                height,
                len = pixels.len(),
                expected,
                "WGC frame has unexpected size; dropping it"
            );
            return Ok(());
        }
        let data = Bytes::copy_from_slice(pixels);

        match lock_slot(&self.shared) {
            Some(mut slot) => {
                *slot = Some(RawFrame::new(width, height, PixelFormat::Bgra, data));
                self.shared.ready.notify_one();
            }
            None => capture_control.stop(),
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        tracing::debug!("WGC capture item closed");
        self.shared.mark_closed();
        Ok(())
    }
}

/// A running Windows Graphics Capture session. Frames arrive on a background thread; the newest one waits in a slot.
pub struct WgcCapture {
    shared: Arc<Shared>,
    control: Option<Control>,
}

impl WgcCapture {
    /// Start capturing. Errors (unsupported Windows, target not found, session refused) are returned, never panicked.
    pub fn start(target: WgcTarget) -> Result<Self, ScreenCaptureError> {
        let shared = Arc::new(Shared::new());

        let control = match target {
            WgcTarget::Monitor(id) => {
                let handle = find_monitor_handle(id)?;
                start_with_border_retry(|border| {
                    let monitor = Monitor::from_raw_hmonitor(handle);
                    FrameHandler::start_free_threaded(settings(monitor, border, shared.clone()))
                })?
            }
            WgcTarget::Window(id) => {
                let handle = find_window_handle(id)?;
                start_with_border_retry(|border| {
                    let window = Window::from_raw_hwnd(handle);
                    FrameHandler::start_free_threaded(settings(window, border, shared.clone()))
                })?
            }
        };

        Ok(Self {
            shared,
            control: Some(control),
        })
    }

    /// Wait up to `timeout` for a frame newer than the last one taken. `None` = nothing new (a still screen sends no frames).
    pub fn take_frame(&self, timeout: Duration) -> Option<RawFrame> {
        let deadline = Instant::now() + timeout;
        let mut slot = lock_slot(&self.shared)?;
        loop {
            if let Some(frame) = slot.take() {
                return Some(frame);
            }
            if self.shared.closed.load(Ordering::SeqCst) {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            slot = match self.shared.ready.wait_timeout(slot, deadline - now) {
                Ok((guard, _)) => guard,
                Err(_) => {
                    tracing::warn!("WGC frame slot wait failed; closing capture session");
                    self.shared.mark_closed();
                    return None;
                }
            };
        }
    }

    /// True once the session ended by itself (window closed, display gone) or failed.
    pub fn is_closed(&self) -> bool {
        if self.shared.closed.load(Ordering::SeqCst) {
            return true;
        }
        match &self.control {
            Some(control) => control.is_finished(),
            None => true,
        }
    }
}

impl Drop for WgcCapture {
    fn drop(&mut self) {
        self.shared.mark_closed();
        if let Some(control) = self.control.take() {
            // `stop` posts WM_QUIT and joins the capture thread. Run it detached
            // so dropping a session never blocks the caller.
            let spawned = thread::Builder::new()
                .name("finch-wgc-stop".to_string())
                .spawn(move || {
                    if let Err(e) = control.stop() {
                        tracing::warn!(error = %e, "WGC session stopped with an error");
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "Could not spawn WGC stop thread");
            }
        }
    }
}

fn settings<T>(
    item: T,
    border: DrawBorderSettings,
    shared: Arc<Shared>,
) -> Settings<Arc<Shared>, T>
where
    T: windows_capture::settings::TryIntoCaptureItemWithType,
{
    Settings::new(
        item,
        CursorCaptureSettings::WithCursor,
        border,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        shared,
    )
}

/// Starts a session without the yellow border; if the OS refuses that, retries once with the default border.
fn start_with_border_retry<E, F>(mut attempt: F) -> Result<Control, ScreenCaptureError>
where
    E: Display,
    F: FnMut(DrawBorderSettings) -> Result<Control, E>,
{
    match attempt(DrawBorderSettings::WithoutBorder) {
        Ok(control) => Ok(control),
        Err(e) => {
            tracing::warn!(error = %e, "WGC refused capture without border; retrying with default border");
            attempt(DrawBorderSettings::Default).map_err(|e| {
                tracing::warn!(error = %e, "WGC session could not be started");
                ScreenCaptureError::XCap(xcap::XCapError::new(format!(
                    "Windows Graphics Capture session could not be started: {e}"
                )))
            })
        }
    }
}

/// Maps a monitor id from `xcap` (`HMONITOR` truncated to u32, see xcap `impl_monitor.rs`) back to its handle.
fn find_monitor_handle(id: u32) -> Result<*mut c_void, ScreenCaptureError> {
    let monitors = Monitor::enumerate().map_err(|e| {
        tracing::warn!(error = %e, "WGC monitor enumeration failed");
        ScreenCaptureError::NoMonitorsAvailable
    })?;
    if monitors.is_empty() {
        return Err(ScreenCaptureError::NoMonitorsAvailable);
    }
    monitors
        .iter()
        .map(Monitor::as_raw_hmonitor)
        .find(|&handle| handle as usize as u32 == id)
        .ok_or_else(|| {
            tracing::warn!(id, "No monitor matches the WGC monitor id");
            ScreenCaptureError::MonitorNotFound(id as usize)
        })
}

/// Maps a window id from `xcap` (`HWND` truncated to u32, see xcap `impl_window.rs`) back to its handle.
fn find_window_handle(id: u32) -> Result<*mut c_void, ScreenCaptureError> {
    let windows = Window::enumerate().map_err(|e| {
        tracing::warn!(error = %e, "WGC window enumeration failed");
        ScreenCaptureError::NoWindowsAvailable
    })?;
    if windows.is_empty() {
        return Err(ScreenCaptureError::NoWindowsAvailable);
    }
    windows
        .iter()
        .map(Window::as_raw_hwnd)
        .find(|&handle| handle as usize as u32 == id)
        .ok_or_else(|| {
            tracing::warn!(id, "No window matches the WGC window id");
            ScreenCaptureError::WindowNotFound(id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_unknown_monitor_is_error() {
        assert!(WgcCapture::start(WgcTarget::Monitor(u32::MAX)).is_err());
    }

    #[test]
    fn start_unknown_window_is_error() {
        assert!(WgcCapture::start(WgcTarget::Window(u32::MAX)).is_err());
    }

    #[test]
    fn take_frame_on_primary_returns_well_formed_frame_or_none() {
        let Ok(monitors) = crate::capture::list_monitors() else {
            return;
        };
        let Some(primary) = monitors.iter().find(|m| m.is_primary) else {
            return;
        };
        // Headless machines cannot start a session; that is not a failure here.
        let Ok(capture) = WgcCapture::start(WgcTarget::Monitor(primary.id)) else {
            return;
        };
        if let Some(frame) = capture.take_frame(Duration::from_millis(500)) {
            assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize);
            assert_eq!(frame.format, PixelFormat::Bgra);
        }
    }
}
