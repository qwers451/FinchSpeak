//! Screen and window capture abstraction using `xcap`.
//!
//! Provides discovery and frame capture for connected monitors and active windows.
//! Captured frames are returned as [`RawFrame`] containing RGBA or BGRA pixel buffers.

use bytes::Bytes;
use thiserror::Error;
use xcap::{Monitor, Window};

/// Errors that can occur during screen or window capture.
#[derive(Debug, Error)]
pub enum ScreenCaptureError {
    /// Underlying XCap library error.
    #[error("XCap capture error: {0}")]
    XCap(#[from] xcap::XCapError),

    /// Specified monitor ID or index was not found.
    #[error("Monitor with id or index {0} not found")]
    MonitorNotFound(usize),

    /// Specified window ID was not found.
    #[error("Window with id {0} not found")]
    WindowNotFound(u32),

    /// No monitors are detected or available on the system.
    #[error("No monitors available for capture")]
    NoMonitorsAvailable,

    /// No windows are detected or available on the system.
    #[error("No windows available for capture")]
    NoWindowsAvailable,

    /// Captured frame has invalid dimensions.
    #[error("Invalid capture dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    /// Captured frame contains no pixel data.
    #[error("Captured frame is empty")]
    EmptyFrame,
}

/// Pixel format of the captured raw frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit RGBA (8 bits per channel: Red, Green, Blue, Alpha).
    Rgba,
    /// 32-bit BGRA (8 bits per channel: Blue, Green, Red, Alpha).
    Bgra,
}

/// Information describing a detected display monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    /// Unique monitor identifier.
    pub id: u32,
    /// Display name of the monitor (e.g. "\\\\.\\DISPLAY1").
    pub name: String,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// Whether this is the primary system monitor.
    pub is_primary: bool,
    /// Display scaling factor (e.g. 1.0 for 100%, 1.25 for 125%).
    pub scale_factor: f32,
    /// Refresh rate frequency in Hz.
    pub frequency: f32,
}

/// Information describing a capturable window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// Unique window handle / ID.
    pub id: u32,
    /// Window title.
    pub title: String,
    /// Owning application name or process name.
    pub app_name: String,
    /// Width of the window in pixels.
    pub width: u32,
    /// Height of the window in pixels.
    pub height: u32,
    /// Whether the window is currently minimized.
    pub is_minimized: bool,
    /// Whether the window is currently maximized.
    pub is_maximized: bool,
}

/// A captured uncompressed video frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel format (typically RGBA).
    pub format: PixelFormat,
    /// Raw uncompressed pixel bytes.
    pub data: Bytes,
}

impl RawFrame {
    /// Creates a new [`RawFrame`].
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Bytes) -> Self {
        Self {
            width,
            height,
            format,
            data,
        }
    }

    /// Converts the pixel buffer to RGBA format (in-place byte swapping if currently BGRA).
    pub fn to_rgba(&self) -> Vec<u8> {
        match self.format {
            PixelFormat::Rgba => self.data.to_vec(),
            PixelFormat::Bgra => {
                let mut rgba = self.data.to_vec();
                for chunk in rgba.as_chunks_mut::<4>().0 {
                    chunk.swap(0, 2);
                }
                rgba
            }
        }
    }

    /// Converts the pixel buffer to BGRA format (in-place byte swapping if currently RGBA).
    pub fn to_bgra(&self) -> Vec<u8> {
        match self.format {
            PixelFormat::Bgra => self.data.to_vec(),
            PixelFormat::Rgba => {
                let mut bgra = self.data.to_vec();
                for chunk in bgra.as_chunks_mut::<4>().0 {
                    chunk.swap(0, 2);
                }
                bgra
            }
        }
    }
}

/// Lists all available monitors on the system.
pub fn list_monitors() -> Result<Vec<MonitorInfo>, ScreenCaptureError> {
    let monitors = Monitor::all()?;
    if monitors.is_empty() {
        return Err(ScreenCaptureError::NoMonitorsAvailable);
    }

    let infos = monitors
        .into_iter()
        .map(|m| MonitorInfo {
            id: m.id(),
            name: m.name().to_string(),
            width: m.width(),
            height: m.height(),
            is_primary: m.is_primary(),
            scale_factor: m.scale_factor(),
            frequency: m.frequency(),
        })
        .collect();

    Ok(infos)
}

/// Captures a single frame from the specified monitor by index or monitor ID.
///
/// Returns a [`RawFrame`] with [`PixelFormat::Rgba`].
pub fn capture_monitor(monitor_id: usize) -> Result<RawFrame, ScreenCaptureError> {
    let monitors = Monitor::all()?;
    if monitors.is_empty() {
        return Err(ScreenCaptureError::NoMonitorsAvailable);
    }

    // Match either by monitor hardware ID or by positional index
    let monitor = monitors
        .iter()
        .find(|m| m.id() as usize == monitor_id)
        .or_else(|| monitors.get(monitor_id))
        .ok_or(ScreenCaptureError::MonitorNotFound(monitor_id))?;

    let img = monitor.capture_image()?;
    let width = img.width();
    let height = img.height();

    if width == 0 || height == 0 {
        return Err(ScreenCaptureError::InvalidDimensions { width, height });
    }

    let raw_bytes = img.into_raw();
    if raw_bytes.is_empty() {
        return Err(ScreenCaptureError::EmptyFrame);
    }

    Ok(RawFrame {
        width,
        height,
        format: PixelFormat::Rgba,
        data: Bytes::from(raw_bytes),
    })
}

/// Lists all capturable application windows currently open.
pub fn list_windows() -> Result<Vec<WindowInfo>, ScreenCaptureError> {
    let windows = Window::all()?;
    let infos = windows
        .into_iter()
        .map(|w| WindowInfo {
            id: w.id(),
            title: w.title().to_string(),
            app_name: w.app_name().to_string(),
            width: w.width(),
            height: w.height(),
            is_minimized: w.is_minimized(),
            is_maximized: w.is_maximized(),
        })
        .collect();

    Ok(infos)
}

/// Captures a single frame from the specified window by ID.
///
/// Returns a [`RawFrame`] with [`PixelFormat::Rgba`].
pub fn capture_window(window_id: u32) -> Result<RawFrame, ScreenCaptureError> {
    let windows = Window::all()?;
    let window = windows
        .iter()
        .find(|w| w.id() == window_id)
        .ok_or(ScreenCaptureError::WindowNotFound(window_id))?;

    let img = window.capture_image()?;
    let width = img.width();
    let height = img.height();

    if width == 0 || height == 0 {
        return Err(ScreenCaptureError::InvalidDimensions { width, height });
    }

    let raw_bytes = img.into_raw();
    if raw_bytes.is_empty() {
        return Err(ScreenCaptureError::EmptyFrame);
    }

    Ok(RawFrame {
        width,
        height,
        format: PixelFormat::Rgba,
        data: Bytes::from(raw_bytes),
    })
}

/// Convenient stateful capturer wrapping the capture functions.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScreenCapturer;

impl ScreenCapturer {
    /// Creates a new [`ScreenCapturer`].
    pub fn new() -> Self {
        Self
    }

    /// Lists all available monitors.
    pub fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ScreenCaptureError> {
        list_monitors()
    }

    /// Captures a frame from a monitor by index or ID.
    pub fn capture_monitor(&self, monitor_id: usize) -> Result<RawFrame, ScreenCaptureError> {
        capture_monitor(monitor_id)
    }

    /// Lists all capturable windows.
    pub fn list_windows(&self) -> Result<Vec<WindowInfo>, ScreenCaptureError> {
        list_windows()
    }

    /// Captures a frame from a window by ID.
    pub fn capture_window(&self, window_id: u32) -> Result<RawFrame, ScreenCaptureError> {
        capture_window(window_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_monitors_or_headless() -> Result<(), Box<dyn std::error::Error>> {
        match list_monitors() {
            Ok(monitors) => {
                assert!(!monitors.is_empty());
                for m in &monitors {
                    assert!(m.width > 0);
                    assert!(m.height > 0);
                }
            }
            Err(ScreenCaptureError::NoMonitorsAvailable) => {
                // Headless environment without displays
            }
            Err(e) => {
                eprintln!("Monitor query in test environment returned: {e}");
            }
        }
        Ok(())
    }

    #[test]
    fn test_capture_monitor_non_existent() {
        let res = capture_monitor(999_999);
        assert!(res.is_err());
    }

    #[test]
    fn test_capture_window_non_existent() {
        let res = capture_window(999_999);
        assert!(res.is_err());
    }

    #[test]
    fn test_raw_frame_bgra_rgba_conversions() {
        // Red pixel (R=255, G=0, B=0, A=255) and Green pixel (R=0, G=255, B=0, A=255)
        let rgba_bytes = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let frame = RawFrame::new(2, 1, PixelFormat::Rgba, Bytes::from(rgba_bytes.clone()));

        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.format, PixelFormat::Rgba);

        // Convert to BGRA: Blue and Red channels swap
        let bgra = frame.to_bgra();
        assert_eq!(bgra, vec![0, 0, 255, 255, 0, 255, 0, 255]);

        let frame_bgra = RawFrame::new(2, 1, PixelFormat::Bgra, Bytes::from(bgra));
        let converted_back = frame_bgra.to_rgba();
        assert_eq!(converted_back, rgba_bytes);
    }
}
