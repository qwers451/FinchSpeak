//! # FinchSpeak Screen
//!
//! Screen capture and video fragmentation for FinchSpeak.
//!
//! - Captures display and window frames (via xcap / WGC).
//! - Encodes H.264 video using OpenH264.
//! - Slices delta frames into QUIC datagrams and assembles them on the receiver.
//! - Integrates with WebCodecs API via Tauri IPC Channel.

pub mod audio_loopback;
pub mod capture;
pub mod fragmenter;
pub mod hw_encoder;
pub mod thumbnail;
pub mod yuv;

pub use audio_loopback::{
    list_loopback_devices, AudioLoopback, LoopbackAudioError, ResamplerConverter,
    ScreenAudioCapturer, ScreenAudioConfig, ScreenAudioEncoder, CHANNELS, DEFAULT_BITRATE_BPS,
    FRAME_DURATION_MS, MAX_OPUS_PAYLOAD_SIZE, SAMPLES_PER_CHANNEL, SAMPLE_RATE,
    STEREO_FRAME_SAMPLES,
};
pub use capture::{
    capture_monitor, capture_window, list_monitors, list_windows, MonitorInfo, PixelFormat,
    RawFrame, ScreenCaptureError, ScreenCapturer, WindowInfo,
};
pub use fragmenter::{slice_p_frame, slice_p_frame_with_chunk, AssembledFrame, FrameAssembler, DELTA_FRAME_TIMEOUT};
pub use thumbnail::{thumbnail_jpeg, ThumbnailCache, MAX_THUMBNAIL_WIDTH};
pub use hw_encoder::{
    EncodedFrame, H264EncoderError, H264VideoEncoder, VideoEncoderConfig, MAX_HEIGHT, MAX_WIDTH,
};
