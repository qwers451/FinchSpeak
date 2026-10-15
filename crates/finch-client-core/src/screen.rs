//! Screen sharing media path (AUD-018): capture frames -> H.264 -> key frame stream / datagrams on
//! the sending side, and the receiving state machine that decides when a frame may be decoded.
//!
//! Everything here is synchronous and network-free so it can be tested without a server; the
//! Tauri app glues it to a [`Session`](crate::session::Session).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use finch_protocol::media::{MediaHeader, MAX_KEYFRAME_BYTES, MEDIA_HEADER_SIZE};
use finch_screen::{
    slice_p_frame_with_chunk, AssembledFrame, FrameAssembler, H264EncoderError, H264VideoEncoder,
    PixelFormat, RawFrame, VideoEncoderConfig, DELTA_FRAME_TIMEOUT,
};
use thiserror::Error;

/// Viewers ask for a new key frame at most this often.
pub const KEYFRAME_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
/// A key frame stream that has not finished after this long is given up on.
pub const KEYFRAME_STREAM_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Error)]
pub enum ScreenError {
    #[error("video encoder: {0}")]
    Encoder(#[from] H264EncoderError),
    #[error("frame is too small to encode ({0}x{1})")]
    FrameTooSmall(u32, u32),
}

/// What to share and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShareConfig {
    pub fps: u8,
    /// `None` picks a bitrate from the picture size.
    pub bitrate_bps: Option<u32>,
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self { fps: 30, bitrate_bps: None }
    }
}

fn default_bitrate(width: u32, height: u32, fps: u8) -> u32 {
    let bits = u64::from(width) * u64::from(height) * u64::from(fps) / 12;
    bits.clamp(1_000_000, 8_000_000) as u32
}

/// One encoded frame, ready to send.
#[derive(Debug)]
pub enum OutgoingVideo {
    /// Send on its own uni-directional stream (`header` first, then `data`).
    KeyFrame { header: MediaHeader, data: Bytes },
    /// Complete datagrams (header + payload), in order.
    Delta(Vec<Bytes>),
    /// The frame could not be sent (too big for the datagram limit); the next one is a key
    /// frame, so viewers resynchronise instead of decoding garbage.
    Dropped,
}

/// Turns raw captured frames into packets for one screen share.
pub struct ScreenPacketizer {
    config: ShareConfig,
    channel_id: u32,
    sender_peer_id: u32,
    encoder: Option<H264VideoEncoder>,
    sequence: u16,
    force_key: bool,
}

impl ScreenPacketizer {
    pub fn new(config: ShareConfig, channel_id: u32, sender_peer_id: u32) -> Self {
        Self { config, channel_id, sender_peer_id, encoder: None, sequence: 0, force_key: false }
    }

    /// The (even) picture size currently being encoded.
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        self.encoder.as_ref().map(|e| (e.width(), e.height()))
    }

    /// The next frame will be a key frame (a viewer asked, or one just joined).
    pub fn request_keyframe(&mut self) {
        self.force_key = true;
    }

    /// Encode `frame`. `timestamp` is in ms since the share started; `max_datagram` is the
    /// largest datagram the connection accepts.
    pub fn push_frame(
        &mut self,
        frame: &RawFrame,
        timestamp: u32,
        max_datagram: usize,
    ) -> Result<OutgoingVideo, ScreenError> {
        // H.264 4:2:0 needs even dimensions.
        let (w, h) = (frame.width & !1, frame.height & !1);
        if w == 0 || h == 0 {
            return Err(ScreenError::FrameTooSmall(frame.width, frame.height));
        }
        if self.dimensions() != Some((w, h)) {
            let bitrate = self
                .config
                .bitrate_bps
                .unwrap_or_else(|| default_bitrate(w, h, self.config.fps));
            self.encoder = Some(H264VideoEncoder::with_config(VideoEncoderConfig {
                width: w,
                height: h,
                fps: self.config.fps,
                bitrate_bps: bitrate,
            })?);
            self.force_key = true;
        }
        let Some(encoder) = self.encoder.as_mut() else {
            return Err(ScreenError::FrameTooSmall(frame.width, frame.height));
        };
        if std::mem::take(&mut self.force_key) {
            encoder.force_keyframe();
        }

        let encoded = encoder.encode_bgra(&crop_to_bgra(frame, w, h))?;

        if encoded.is_keyframe {
            if encoded.data.len() > MAX_KEYFRAME_BYTES {
                self.force_key = true;
                return Ok(OutgoingVideo::Dropped);
            }
            let header = MediaHeader::new_video_key(
                self.channel_id,
                self.sender_peer_id,
                self.next_sequence(1),
                timestamp,
                encoded.frame_id,
                false,
            );
            return Ok(OutgoingVideo::KeyFrame { header, data: encoded.data });
        }

        let chunk = max_datagram.saturating_sub(MEDIA_HEADER_SIZE);
        let packets = slice_p_frame_with_chunk(
            self.channel_id,
            self.sender_peer_id,
            self.sequence,
            encoded.frame_id,
            timestamp,
            &encoded.data,
            false,
            chunk,
        );
        match packets {
            Some(packets) if !packets.is_empty() => {
                self.sequence = self.sequence.wrapping_add(packets.len() as u16);
                Ok(OutgoingVideo::Delta(packets.iter().map(|p| p.encode()).collect()))
            }
            _ => {
                self.force_key = true;
                Ok(OutgoingVideo::Dropped)
            }
        }
    }

    fn next_sequence(&mut self, used: u16) -> u16 {
        let current = self.sequence;
        self.sequence = self.sequence.wrapping_add(used);
        current
    }
}

/// BGRA pixels of the top-left `w`x`h` part of `frame`.
fn crop_to_bgra(frame: &RawFrame, w: u32, h: u32) -> Vec<u8> {
    let bgra = match frame.format {
        PixelFormat::Bgra => frame.data.to_vec(),
        PixelFormat::Rgba => frame.to_bgra(),
    };
    if (w, h) == (frame.width, frame.height) {
        return bgra;
    }
    let (src_stride, row) = (frame.width as usize * 4, w as usize * 4);
    let mut out = Vec::with_capacity(row * h as usize);
    for y in 0..h as usize {
        out.extend_from_slice(&bgra[y * src_stride..y * src_stride + row]);
    }
    out
}

/// Receiving side: decides which frames may be decoded and when to ask for a key frame.
///
/// A delta frame is only decodable if every frame since the last key frame arrived, so any gap
/// (a lost datagram, a frame that never completed, a key frame still in flight) makes the viewer
/// drop deltas and ask the sender for a new key frame.
pub struct ScreenViewer {
    assembler: FrameAssembler,
    /// Id of the frame the next delta must have.
    expected: Option<u16>,
    needs_key: bool,
    last_request: Option<Instant>,
    stream_started: Option<Instant>,
}

impl Default for ScreenViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenViewer {
    pub fn new() -> Self {
        Self {
            assembler: FrameAssembler::new(),
            expected: None,
            needs_key: true,
            last_request: None,
            stream_started: None,
        }
    }

    /// True when a `RequestKeyframe` should be sent now. Rate limited; keeps returning true
    /// (once per interval) until a key frame arrives.
    pub fn take_keyframe_request(&mut self, now: Instant) -> bool {
        if !self.needs_key {
            return false;
        }
        if self.last_request.is_some_and(|at| now.duration_since(at) < KEYFRAME_REQUEST_INTERVAL) {
            return false;
        }
        self.last_request = Some(now);
        true
    }

    /// A key frame stream was opened: deltas received until it finishes cannot be decoded.
    pub fn keyframe_stream_started(&mut self, now: Instant) {
        self.assembler.on_keyframe_stream_started();
        self.stream_started = Some(now);
    }

    pub fn on_keyframe(&mut self, header: &MediaHeader, data: Bytes, _now: Instant) -> AssembledFrame {
        self.needs_key = false;
        self.stream_started = None;
        self.last_request = None;
        self.expected = Some(header.frame_id.wrapping_add(1));
        self.assembler.on_keyframe_received(header.frame_id, header.timestamp, data)
    }

    /// A delta fragment arrived; returns the frame once complete and decodable.
    pub fn on_delta_fragment(
        &mut self,
        header: &MediaHeader,
        payload: Bytes,
        now: Instant,
    ) -> Option<AssembledFrame> {
        if self.needs_key || self.stream_started.is_some() {
            return None;
        }
        let expected = self.expected?;
        let ahead = header.frame_id.wrapping_sub(expected) as i16;
        if ahead < 0 {
            return None; // a late fragment of a frame we already handled
        }
        if ahead > 0 {
            self.lose_sync();
            return None;
        }
        let frame = self.assembler.push_fragment(header, payload, now)?;
        self.expected = Some(frame.frame_id.wrapping_add(1));
        Some(frame)
    }

    /// Housekeeping: call every few tens of milliseconds.
    pub fn tick(&mut self, now: Instant) {
        let timed_out_frames = !self.assembler.prune_stale(now, DELTA_FRAME_TIMEOUT).is_empty();
        let stream_stuck = self
            .stream_started
            .is_some_and(|at| now.duration_since(at) > KEYFRAME_STREAM_TIMEOUT);
        if timed_out_frames || stream_stuck {
            self.lose_sync();
        }
    }

    fn lose_sync(&mut self) {
        self.needs_key = true;
        self.expected = None;
        self.stream_started = None;
        self.assembler = FrameAssembler::new();
    }
}

// ---- sending side driver --------------------------------------------------------------------

/// Where frames come from: a monitor, a window, or (in tests) a generator.
pub trait FrameSource: Send + 'static {
    fn next_frame(&mut self) -> Result<RawFrame, String>;
}

/// Captures a whole monitor.
pub struct MonitorSource(pub usize);

impl FrameSource for MonitorSource {
    fn next_frame(&mut self) -> Result<RawFrame, String> {
        finch_screen::capture_monitor(self.0).map_err(|e| e.to_string())
    }
}

/// Captures one window.
pub struct WindowSource(pub u32);

impl FrameSource for WindowSource {
    fn next_frame(&mut self) -> Result<RawFrame, String> {
        finch_screen::capture_window(self.0).map_err(|e| e.to_string())
    }
}

/// Consecutive capture failures (window closed, display off) before the share ends.
const MAX_CAPTURE_FAILURES: u32 = 30;

/// A running screen share: a capture thread that encodes and sends until stopped.
pub struct ScreenSharer {
    stop: Arc<AtomicBool>,
    keyframe: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ScreenSharer {
    /// Start sharing on `connection`. `on_end` runs (on the capture thread) with the reason if
    /// the share ends by itself - capture keeps failing, the connection is gone - but not after
    /// [`stop`](Self::stop).
    pub fn start(
        connection: quinn::Connection,
        runtime: tokio::runtime::Handle,
        channel_id: u32,
        sender_peer_id: u32,
        config: ShareConfig,
        mut source: Box<dyn FrameSource>,
        on_end: impl FnOnce(String) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let keyframe = Arc::new(AtomicBool::new(true));
        let (stop_flag, key_flag) = (stop.clone(), keyframe.clone());

        let thread = std::thread::Builder::new()
            .name("screen-share".into())
            .spawn(move || {
                let mut packetizer = ScreenPacketizer::new(config, channel_id, sender_peer_id);
                let interval = Duration::from_millis(1000 / u64::from(config.fps.max(1)));
                let started = Instant::now();
                let mut failures = 0u32;
                let reason = loop {
                    if stop_flag.load(Ordering::Relaxed) {
                        return;
                    }
                    let frame_start = Instant::now();
                    if key_flag.swap(false, Ordering::Relaxed) {
                        packetizer.request_keyframe();
                    }
                    match source.next_frame() {
                        Ok(frame) => {
                            failures = 0;
                            let Some(max_datagram) = connection.max_datagram_size() else {
                                break "the connection does not support datagrams".to_string();
                            };
                            let timestamp = started.elapsed().as_millis() as u32;
                            match packetizer.push_frame(&frame, timestamp, max_datagram) {
                                Ok(OutgoingVideo::KeyFrame { header, data }) => {
                                    let connection = connection.clone();
                                    runtime.spawn(async move {
                                        let _ = crate::session::send_keyframe_on(&connection, &header, &data).await;
                                    });
                                }
                                Ok(OutgoingVideo::Delta(datagrams)) => {
                                    for datagram in datagrams {
                                        if let Err(e) = connection.send_datagram(datagram) {
                                            if connection.close_reason().is_some() {
                                                break;
                                            }
                                            // Congested or oversized: viewers need a new key frame.
                                            tracing::debug!("screen share datagram dropped: {e}");
                                            packetizer.request_keyframe();
                                            break;
                                        }
                                    }
                                }
                                Ok(OutgoingVideo::Dropped) => {}
                                Err(e) => break e.to_string(),
                            }
                        }
                        Err(e) => {
                            failures += 1;
                            if failures >= MAX_CAPTURE_FAILURES {
                                break format!("screen capture keeps failing: {e}");
                            }
                        }
                    }
                    if connection.close_reason().is_some() {
                        break "connection closed".to_string();
                    }
                    if let Some(rest) = interval.checked_sub(frame_start.elapsed()) {
                        std::thread::sleep(rest);
                    }
                };
                if !stop_flag.load(Ordering::Relaxed) {
                    on_end(reason);
                }
            })
            .ok();

        Self { stop, keyframe, thread }
    }

    /// A viewer needs a key frame.
    pub fn request_keyframe(&self) {
        self.keyframe.store(true, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Stop sharing and wait for the capture thread.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ScreenSharer {
    fn drop(&mut self) {
        self.stop();
    }
}
