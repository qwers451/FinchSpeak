//! # FinchSpeak Screen
//!
//! Screen capture and video fragmentation for FinchSpeak.
//!
//! - Captures display and window frames (via xcap / WGC).
//! - Encodes H.264 video.
//! - Slices delta frames into QUIC datagrams and assembles them on the receiver.
//! - Integrates with WebCodecs API via Tauri IPC Channel.

pub mod fragmenter;

pub use fragmenter::{slice_p_frame, AssembledFrame, FrameAssembler, DELTA_FRAME_TIMEOUT};
