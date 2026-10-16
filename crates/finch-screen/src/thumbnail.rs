//! Small JPEG previews of monitors and windows for the "what do you want to share" picker.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageBuffer, Rgb, Rgba};

use crate::capture::RawFrame;

/// Previews are never wider than this, whatever the caller asks for.
pub const MAX_THUMBNAIL_WIDTH: u32 = 640;
/// JPEG quality: previews are tiny, so size matters more than detail.
const JPEG_QUALITY: u8 = 70;
/// Asking again within this time returns the previous picture instead of capturing again.
pub const MIN_CAPTURE_INTERVAL: Duration = Duration::from_millis(400);

/// Scale `frame` down to `max_width` (never up) and encode it as JPEG.
pub fn thumbnail_jpeg(frame: &RawFrame, max_width: u32) -> Result<Vec<u8>, String> {
    let (width, height) = (frame.width, frame.height);
    if width == 0 || height == 0 {
        return Err("empty frame".to_string());
    }
    if frame.data.len() < (width as usize) * (height as usize) * 4 {
        return Err("frame data is shorter than its size says".to_string());
    }
    let rgba = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, frame.to_rgba()).ok_or("bad frame size")?;
    let target_width = max_width.clamp(16, MAX_THUMBNAIL_WIDTH).min(width);
    let target_height = ((u64::from(height) * u64::from(target_width)) / u64::from(width)).max(1) as u32;
    let small = image::imageops::resize(&rgba, target_width, target_height, FilterType::Triangle);
    // JPEG has no alpha.
    let rgb: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(small.width(), small.height(), |x, y| {
        let p = small.get_pixel(x, y);
        Rgb([p[0], p[1], p[2]])
    });
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Remembers the last preview of every source so a picker that asks every second does not make
/// the machine capture every second for every tile.
#[derive(Default)]
pub struct ThumbnailCache {
    entries: HashMap<(bool, u32), (Instant, Vec<u8>)>,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The preview of source (`window`, `id`): the cached one if it is fresh, else `capture()`.
    pub fn get_or_capture(
        &mut self,
        window: bool,
        id: u32,
        now: Instant,
        capture: impl FnOnce() -> Result<Vec<u8>, String>,
    ) -> Result<Vec<u8>, String> {
        if let Some((taken, bytes)) = self.entries.get(&(window, id))
            && now.duration_since(*taken) < MIN_CAPTURE_INTERVAL
        {
            return Ok(bytes.clone());
        }
        let bytes = capture()?;
        self.entries.insert((window, id), (now, bytes.clone()));
        // Sources come and go; keep the table small.
        if self.entries.len() > 64 {
            self.entries.retain(|_, (taken, _)| now.duration_since(*taken) < Duration::from_secs(30));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::PixelFormat;
    use bytes::Bytes;

    fn frame(width: u32, height: u32, format: PixelFormat) -> RawFrame {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 40, 255]);
            }
        }
        RawFrame::new(width, height, format, Bytes::from(data))
    }

    #[test]
    fn a_big_frame_becomes_a_small_jpeg_with_the_same_proportions() {
        let jpeg = thumbnail_jpeg(&frame(1920, 1080, PixelFormat::Rgba), 320).unwrap();
        assert_eq!(&jpeg[..3], &[0xFF, 0xD8, 0xFF], "JPEG signature");
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 180));
        assert!(jpeg.len() < 40_000, "a preview is small, got {} bytes", jpeg.len());
    }

    #[test]
    fn small_frames_are_not_enlarged_and_limits_hold() {
        let small = thumbnail_jpeg(&frame(200, 100, PixelFormat::Bgra), 320).unwrap();
        let decoded = image::load_from_memory(&small).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (200, 100));

        let capped = thumbnail_jpeg(&frame(2000, 1000, PixelFormat::Rgba), 5000).unwrap();
        assert_eq!(image::load_from_memory(&capped).unwrap().width(), MAX_THUMBNAIL_WIDTH);
    }

    #[test]
    fn bgra_and_rgba_give_the_same_picture() {
        let rgba = frame(64, 64, PixelFormat::Rgba);
        let bgra = RawFrame::new(64, 64, PixelFormat::Bgra, Bytes::from(rgba.to_bgra()));
        assert_eq!(thumbnail_jpeg(&rgba, 64).unwrap(), thumbnail_jpeg(&bgra, 64).unwrap());
    }

    #[test]
    fn broken_frames_are_errors_not_panics() {
        assert!(thumbnail_jpeg(&RawFrame::new(0, 0, PixelFormat::Rgba, Bytes::new()), 320).is_err());
        assert!(thumbnail_jpeg(&RawFrame::new(100, 100, PixelFormat::Rgba, Bytes::from(vec![0u8; 10])), 320).is_err());
    }

    #[test]
    fn the_cache_answers_quick_repeats_and_refreshes_later() {
        let mut cache = ThumbnailCache::new();
        let start = Instant::now();
        let mut captures = 0;
        let mut take = |n: u8| { captures += 1; Ok(vec![n]) };
        assert_eq!(cache.get_or_capture(false, 1, start, || take(1)).unwrap(), vec![1]);
        assert_eq!(cache.get_or_capture(false, 1, start + Duration::from_millis(100), || take(2)).unwrap(), vec![1]);
        assert_eq!(cache.get_or_capture(true, 1, start + Duration::from_millis(100), || take(3)).unwrap(), vec![3], "a window is not a monitor with the same id");
        assert_eq!(cache.get_or_capture(false, 1, start + Duration::from_millis(500), || take(4)).unwrap(), vec![4]);
        assert_eq!(captures, 3);
    }

    #[test]
    fn a_failed_capture_is_not_remembered() {
        let mut cache = ThumbnailCache::new();
        let now = Instant::now();
        assert!(cache.get_or_capture(false, 1, now, || Err("gone".to_string())).is_err());
        assert_eq!(cache.get_or_capture(false, 1, now, || Ok(vec![9])).unwrap(), vec![9]);
    }
}
