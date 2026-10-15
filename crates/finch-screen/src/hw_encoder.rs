//! H.264 video encoder based on Cisco's OpenH264.
//!
//! Encodes raw video frames (YUV420p or RGBA/BGRA) into H.264 bitstreams (Annex B format).
//! IDR keyframes are detected and marked accordingly for transmission over QUIC Uni-Streams,
//! while delta (P) frames are fragmented into datagrams.

use bytes::Bytes;
use openh264::encoder::{Encoder, EncoderConfig, FrameType, RateControlMode, UsageType};
use openh264::formats::{BgraSliceU8, RgbaSliceU8, YUVBuffer, YUVSlices};
use openh264::{OpenH264API, Timestamp};
use thiserror::Error;

/// Maximum resolution supported by OpenH264 (Level 5.2).
pub const MAX_WIDTH: u32 = 3840;
pub const MAX_HEIGHT: u32 = 2160;

/// Errors that can occur during H.264 video encoding.
#[derive(Debug, Error)]
pub enum H264EncoderError {
    /// Underling OpenH264 encoder error.
    #[error("OpenH264 error: {0}")]
    OpenH264(#[from] openh264::Error),

    /// Invalid frame dimensions (must be even, > 0, and <= 3840x2160).
    #[error("Invalid video dimensions: {width}x{height}. Both must be even, non-zero, and at most {MAX_WIDTH}x{MAX_HEIGHT}")]
    InvalidDimensions { width: u32, height: u32 },

    /// Buffer size mismatch for the given frame dimensions.
    #[error("Buffer size mismatch: expected {expected} bytes for {width}x{height}, got {actual} bytes")]
    InvalidBufferSize {
        expected: usize,
        actual: usize,
        width: u32,
        height: u32,
    },

    /// Encoder returned an invalid or unencoded frame.
    #[error("Encoder returned an invalid frame")]
    InvalidFrame,

    /// Encoder produced an empty bitstream.
    #[error("Encoder produced empty bitstream")]
    EmptyBitstream,
}

/// Configuration parameters for [`H264VideoEncoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoEncoderConfig {
    /// Video frame width in pixels (must be a multiple of 2).
    pub width: u32,
    /// Video frame height in pixels (must be a multiple of 2).
    pub height: u32,
    /// Target frame rate in frames per second (fps).
    pub fps: u8,
    /// Target bitrate in bits per second (bps).
    pub bitrate_bps: u32,
}

impl Default for VideoEncoderConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_bps: 2_000_000, // 2 Mbps
        }
    }
}

/// An encoded H.264 video frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    /// Indicates whether this frame is an IDR keyframe (I-frame).
    pub is_keyframe: bool,
    /// Annex B encoded H.264 NAL units.
    pub data: Bytes,
    /// Sequential frame identifier (monotonically incrementing u16 counter).
    pub frame_id: u16,
}

/// H.264 video encoder wrapping `openh264::encoder::Encoder`.
pub struct H264VideoEncoder {
    encoder: Encoder,
    config: VideoEncoderConfig,
    next_frame_id: u16,
    frame_count: u64,
    force_keyframe_pending: bool,
    is_initialized: bool,
}

impl H264VideoEncoder {
    /// Creates a new H.264 video encoder with the specified dimensions, FPS, and bitrate.
    pub fn new(
        width: u32,
        height: u32,
        fps: u8,
        bitrate_bps: u32,
    ) -> Result<Self, H264EncoderError> {
        let config = VideoEncoderConfig {
            width,
            height,
            fps,
            bitrate_bps,
        };
        Self::with_config(config)
    }

    /// Creates a new H.264 video encoder with the provided [`VideoEncoderConfig`].
    pub fn with_config(config: VideoEncoderConfig) -> Result<Self, H264EncoderError> {
        validate_dimensions(config.width, config.height)?;

        let openh264_config = EncoderConfig::new()
            .set_bitrate_bps(config.bitrate_bps)
            .max_frame_rate(config.fps.max(1) as f32)
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .enable_skip_frame(false);

        let encoder = Encoder::with_api_config(OpenH264API::from_source(), openh264_config)?;

        Ok(Self {
            encoder,
            config,
            next_frame_id: 0,
            frame_count: 0,
            force_keyframe_pending: false,
            is_initialized: false,
        })
    }

    /// Returns the current encoder configuration.
    #[inline]
    pub fn config(&self) -> &VideoEncoderConfig {
        &self.config
    }

    /// Returns the current configured frame width.
    #[inline]
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// Returns the current configured frame height.
    #[inline]
    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// Returns the configured target frames per second.
    #[inline]
    pub fn fps(&self) -> u8 {
        self.config.fps
    }

    /// Returns the configured target bitrate in bps.
    #[inline]
    pub fn bitrate_bps(&self) -> u32 {
        self.config.bitrate_bps
    }

    /// Returns the next sequential frame ID.
    #[inline]
    pub fn next_frame_id(&self) -> u16 {
        self.next_frame_id
    }

    /// Requests that the next encoded frame be an IDR keyframe.
    pub fn force_keyframe(&mut self) {
        self.force_keyframe_pending = true;
        if self.is_initialized {
            self.encoder.force_intra_frame();
        }
    }

    /// Dynamically updates the video dimensions.
    pub fn set_dimensions(&mut self, width: u32, height: u32) -> Result<(), H264EncoderError> {
        if self.config.width == width && self.config.height == height {
            return Ok(());
        }
        validate_dimensions(width, height)?;
        self.config.width = width;
        self.config.height = height;
        self.force_keyframe();
        Ok(())
    }

    /// Encodes a raw YUV420p (I420) planar frame.
    ///
    /// The buffer size must be exactly `(width * height * 3) / 2` bytes, formatted as:
    /// - Y plane: `width * height` bytes
    /// - U plane: `(width / 2) * (height / 2)` bytes
    /// - V plane: `(width / 2) * (height / 2)` bytes
    pub fn encode_frame(&mut self, yuv_i420: &[u8]) -> Result<EncodedFrame, H264EncoderError> {
        let w = self.config.width as usize;
        let h = self.config.height as usize;
        let expected_size = (w * h * 3) / 2;

        if yuv_i420.len() != expected_size {
            return Err(H264EncoderError::InvalidBufferSize {
                expected: expected_size,
                actual: yuv_i420.len(),
                width: self.config.width,
                height: self.config.height,
            });
        }

        let y_len = w * h;
        let uv_len = (w / 2) * (h / 2);
        let y_plane = &yuv_i420[0..y_len];
        let u_plane = &yuv_i420[y_len..y_len + uv_len];
        let v_plane = &yuv_i420[y_len + uv_len..y_len + 2 * uv_len];

        let yuv_slices = YUVSlices::new((y_plane, u_plane, v_plane), (w, h), (w, w / 2, w / 2));

        self.encode_yuv_source(&yuv_slices)
    }

    /// Encodes an RGBA pixel buffer of dimensions `(width, height)`.
    ///
    /// The buffer size must be exactly `width * height * 4` bytes.
    pub fn encode_rgba(&mut self, rgba: &[u8]) -> Result<EncodedFrame, H264EncoderError> {
        let w = self.config.width as usize;
        let h = self.config.height as usize;
        let expected_size = w * h * 4;

        if rgba.len() != expected_size {
            return Err(H264EncoderError::InvalidBufferSize {
                expected: expected_size,
                actual: rgba.len(),
                width: self.config.width,
                height: self.config.height,
            });
        }

        let rgba_slice = RgbaSliceU8::new(rgba, (w, h));
        let yuv_buffer = YUVBuffer::from_rgb_source(rgba_slice);
        self.encode_yuv_source(&yuv_buffer)
    }

    /// Encodes a BGRA pixel buffer of dimensions `(width, height)`.
    ///
    /// The buffer size must be exactly `width * height * 4` bytes.
    pub fn encode_bgra(&mut self, bgra: &[u8]) -> Result<EncodedFrame, H264EncoderError> {
        let w = self.config.width as usize;
        let h = self.config.height as usize;
        let expected_size = w * h * 4;

        if bgra.len() != expected_size {
            return Err(H264EncoderError::InvalidBufferSize {
                expected: expected_size,
                actual: bgra.len(),
                width: self.config.width,
                height: self.config.height,
            });
        }

        let bgra_slice = BgraSliceU8::new(bgra, (w, h));
        let yuv_buffer = YUVBuffer::from_rgb_source(bgra_slice);
        self.encode_yuv_source(&yuv_buffer)
    }

    /// Internal helper to encode any type implementing [`openh264::formats::YUVSource`].
    fn encode_yuv_source<T: openh264::formats::YUVSource>(
        &mut self,
        yuv: &T,
    ) -> Result<EncodedFrame, H264EncoderError> {
        if self.force_keyframe_pending {
            self.encoder.force_intra_frame();
            self.force_keyframe_pending = false;
        }

        let fps = (self.config.fps as u64).max(1);
        let timestamp_ms = self.frame_count.saturating_mul(1000) / fps;
        let timestamp = Timestamp::from_millis(timestamp_ms);

        let bitstream = self.encoder.encode_at(yuv, timestamp)?;
        let frame_type = bitstream.frame_type();

        if frame_type == FrameType::Invalid {
            return Err(H264EncoderError::InvalidFrame);
        }

        let encoded_bytes = bitstream.to_vec();
        if encoded_bytes.is_empty() {
            return Err(H264EncoderError::EmptyBitstream);
        }

        let is_keyframe = matches!(frame_type, FrameType::IDR | FrameType::I);

        let frame_id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1);
        self.frame_count = self.frame_count.wrapping_add(1);
        self.is_initialized = true;

        Ok(EncodedFrame {
            is_keyframe,
            data: Bytes::from(encoded_bytes),
            frame_id,
        })
    }
}

/// Helper function to validate video frame dimensions.
fn validate_dimensions(width: u32, height: u32) -> Result<(), H264EncoderError> {
    if width == 0
        || height == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || width > MAX_WIDTH
        || height > MAX_HEIGHT
    {
        return Err(H264EncoderError::InvalidDimensions { width, height });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates a test YUV420p pattern of given dimensions.
    fn generate_test_yuv(width: u32, height: u32, pattern_val: u8) -> Vec<u8> {
        let size = (width as usize * height as usize * 3) / 2;
        vec![pattern_val; size]
    }

    #[test]
    fn test_encoder_creation_and_validation() {
        // Valid creation
        let encoder = H264VideoEncoder::new(320, 240, 30, 500_000);
        assert!(encoder.is_ok());

        // Odd width
        let err_odd_w = H264VideoEncoder::new(321, 240, 30, 500_000);
        assert!(matches!(
            err_odd_w,
            Err(H264EncoderError::InvalidDimensions { .. })
        ));

        // Odd height
        let err_odd_h = H264VideoEncoder::new(320, 241, 30, 500_000);
        assert!(matches!(
            err_odd_h,
            Err(H264EncoderError::InvalidDimensions { .. })
        ));

        // Zero dimension
        let err_zero = H264VideoEncoder::new(0, 240, 30, 500_000);
        assert!(matches!(
            err_zero,
            Err(H264EncoderError::InvalidDimensions { .. })
        ));

        // Excessively large dimension
        let err_huge = H264VideoEncoder::new(4000, 3000, 30, 500_000);
        assert!(matches!(
            err_huge,
            Err(H264EncoderError::InvalidDimensions { .. })
        ));
    }

    #[test]
    fn test_encode_yuv_frames_and_keyframes() -> Result<(), Box<dyn std::error::Error>> {
        let width = 128;
        let height = 96;
        let mut encoder = H264VideoEncoder::new(width, height, 30, 200_000)?;

        let yuv_frame1 = generate_test_yuv(width, height, 128);
        let frame1 = encoder.encode_frame(&yuv_frame1)?;

        // The very first frame must be an IDR keyframe
        assert!(
            frame1.is_keyframe,
            "Expected first encoded frame to be a keyframe"
        );
        assert_eq!(frame1.frame_id, 0);
        assert!(!frame1.data.is_empty());
        // Verify Annex B start code prefix 0x00, 0x00, 0x00, 0x01 or 0x00, 0x00, 0x01
        assert!(
            frame1.data.starts_with(&[0, 0, 0, 1]) || frame1.data.starts_with(&[0, 0, 1]),
            "Frame should start with Annex B NAL start code"
        );

        // Second frame should be a delta (P) frame
        let yuv_frame2 = generate_test_yuv(width, height, 130);
        let frame2 = encoder.encode_frame(&yuv_frame2)?;
        assert_eq!(frame2.frame_id, 1);
        assert!(!frame2.data.is_empty());

        // Now force a keyframe
        encoder.force_keyframe();
        let yuv_frame3 = generate_test_yuv(width, height, 132);
        let frame3 = encoder.encode_frame(&yuv_frame3)?;
        assert!(
            frame3.is_keyframe,
            "Expected frame after force_keyframe() to be a keyframe"
        );
        assert_eq!(frame3.frame_id, 2);

        Ok(())
    }

    #[test]
    fn test_encode_rgba_frame() -> Result<(), Box<dyn std::error::Error>> {
        let width = 64;
        let height = 64;
        let mut encoder = H264VideoEncoder::new(width, height, 30, 150_000)?;

        let rgba_data = vec![200u8; (width * height * 4) as usize];
        let frame = encoder.encode_rgba(&rgba_data)?;

        assert!(frame.is_keyframe);
        assert_eq!(frame.frame_id, 0);
        assert!(!frame.data.is_empty());

        Ok(())
    }

    #[test]
    fn test_encode_invalid_buffer_size() -> Result<(), Box<dyn std::error::Error>> {
        let mut encoder = H264VideoEncoder::new(64, 64, 30, 100_000)?;
        let small_buffer = vec![0u8; 10];
        let res = encoder.encode_frame(&small_buffer);
        assert!(matches!(
            res,
            Err(H264EncoderError::InvalidBufferSize { .. })
        ));
        Ok(())
    }
}
