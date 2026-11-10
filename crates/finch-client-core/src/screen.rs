//! Screen sharing media path (AUD-018): capture frames -> H.264 -> key frame stream / datagrams on
//! the sending side, and the receiving state machine that decides when a frame may be decoded.
//!
//! Everything here is synchronous and network-free so it can be tested without a server; the
//! Tauri app glues it to a [`Session`](crate::session::Session).

use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use finch_protocol::media::{MediaHeader, MediaPacket, MAX_KEYFRAME_BYTES, MEDIA_HEADER_SIZE};
use finch_screen::{
    slice_p_frame_with_chunk, AssembledFrame, FrameAssembler, H264EncoderError, H264VideoEncoder,
    PixelFormat, RawFrame, VideoEncoderConfig, DELTA_FRAME_TIMEOUT, MAX_HEIGHT, MAX_WIDTH,
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

/// Largest picture for shares up to 30 FPS; faster shares are limited to 1080p.
pub const MAX_WIDTH_60: u32 = 1920;
pub const MAX_HEIGHT_60: u32 = 1080;

impl ShareConfig {
    /// The largest picture this share sends.
    pub fn size_limit(&self) -> (u32, u32) {
        if self.fps > 30 {
            (MAX_WIDTH_60, MAX_HEIGHT_60)
        } else {
            (MAX_WIDTH, MAX_HEIGHT)
        }
    }
}

fn default_bitrate(width: u32, height: u32, fps: u8) -> u32 {
    if fps > 30 {
        let bits = u64::from(width) * u64::from(height) * u64::from(fps) / 18;
        return bits.clamp(1_000_000, 10_000_000) as u32;
    }
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
    /// Current target rate (`None` until the first frame says how big the picture is).
    bitrate_bps: Option<u32>,
    /// Rate the share started with; recovery never goes above it.
    start_bitrate_bps: Option<u32>,
    /// When viewers recently reported losses (ms on the share's clock).
    losses_ms: VecDeque<u32>,
    /// When the rate last changed or the last loss happened.
    last_event_ms: u32,
    /// Picture limit, fixed by the frame rate the share started with.
    size_limit: (u32, u32),
    /// The bitrate was picked from the picture size (not given in the config).
    auto_bitrate: bool,
}

/// This many reports within [`LOSS_WINDOW_MS`] lower the bitrate.
const LOSSES_TO_DEGRADE: usize = 3;
const LOSS_WINDOW_MS: u32 = 10_000;
/// Calm for this long raises the bitrate again.
const CALM_BEFORE_RECOVERY_MS: u32 = 30_000;
const MIN_BITRATE_BPS: u32 = 500_000;

impl ScreenPacketizer {
    pub fn new(config: ShareConfig, channel_id: u32, sender_peer_id: u32) -> Self {
        Self {
            config,
            channel_id,
            sender_peer_id,
            encoder: None,
            sequence: 0,
            force_key: false,
            bitrate_bps: config.bitrate_bps,
            start_bitrate_bps: config.bitrate_bps,
            losses_ms: VecDeque::new(),
            last_event_ms: 0,
            size_limit: config.size_limit(),
            auto_bitrate: config.bitrate_bps.is_none(),
        }
    }

    /// The bitrate the encoder is currently asked for.
    pub fn bitrate_bps(&self) -> Option<u32> {
        self.bitrate_bps
    }

    /// Change the frame rate of the share. The next frame starts a new encoder (with a key
    /// frame); an automatically chosen bitrate is picked again for the new rate.
    pub fn set_fps(&mut self, fps: u8) {
        self.config.fps = fps;
        self.encoder = None;
        if self.auto_bitrate {
            self.bitrate_bps = None;
            self.start_bitrate_bps = None;
        }
    }

    /// A viewer reported that it lost part of the stream (`at_ms` on the share's clock).
    /// Repeated reports mean the link cannot carry the bitrate: lower it.
    pub fn report_loss(&mut self, at_ms: u32) {
        self.losses_ms.push_back(at_ms);
        self.last_event_ms = at_ms;
        while self.losses_ms.front().is_some_and(|&t| at_ms.wrapping_sub(t) > LOSS_WINDOW_MS) {
            self.losses_ms.pop_front();
        }
        if self.losses_ms.len() >= LOSSES_TO_DEGRADE {
            self.losses_ms.clear();
            if let Some(rate) = self.bitrate_bps {
                self.set_bitrate((rate / 4 * 3).max(MIN_BITRATE_BPS));
            }
        }
    }

    fn set_bitrate(&mut self, rate: u32) {
        if self.bitrate_bps != Some(rate) {
            self.bitrate_bps = Some(rate);
            // A new rate means a new encoder, which starts with a key frame.
            self.encoder = None;
        }
    }

    fn maybe_recover(&mut self, now_ms: u32) {
        let (Some(rate), Some(start)) = (self.bitrate_bps, self.start_bitrate_bps) else {
            return;
        };
        if rate < start && now_ms.wrapping_sub(self.last_event_ms) >= CALM_BEFORE_RECOVERY_MS {
            self.last_event_ms = now_ms;
            self.set_bitrate((rate / 4 * 5).min(start));
        }
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
        // H.264 4:2:0 needs even dimensions, and the encoder has an upper limit.
        let (w, h) = encoded_size_within(frame.width, frame.height, self.size_limit.0, self.size_limit.1);
        if w == 0 || h == 0 {
            return Err(ScreenError::FrameTooSmall(frame.width, frame.height));
        }
        self.maybe_recover(timestamp);
        if self.dimensions() != Some((w, h)) || self.encoder.is_none() {
            let bitrate = match self.bitrate_bps {
                Some(rate) => rate,
                None => {
                    // Start from a rate suited to the picture size.
                    let rate = default_bitrate(w, h, self.config.fps);
                    self.bitrate_bps = Some(rate);
                    self.start_bitrate_bps = Some(rate);
                    rate
                }
            };
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

        let picture = fit_to_bgra(frame, w, h);
        let encoded = encoder.encode_bgra(&picture)?;

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

/// The picture size sent for a `width`x`height` screen: even, and scaled down (keeping the
/// shape) when it is larger than the encoder supports.
pub fn encoded_size(width: u32, height: u32) -> (u32, u32) {
    encoded_size_within(width, height, MAX_WIDTH, MAX_HEIGHT)
}

/// [`encoded_size`] with the picture limits given: even, shape kept, never larger than the
/// limits, and at least 2 pixels per side for inputs of at least 2 pixels.
pub fn encoded_size_within(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width <= max_width && height <= max_height {
        return (width & !1, height & !1);
    }
    let scale = (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    let fit = |side: u32, limit: u32| (((f64::from(side) * scale).round() as u32).max(2) & !1).min(limit);
    (fit(width, max_width), fit(height, max_height))
}

/// The picture size a `width`x`height` screen is shared at when the share runs at `fps`.
pub fn share_size(width: u32, height: u32, fps: u8) -> (u32, u32) {
    let (max_width, max_height) = ShareConfig { fps, bitrate_bps: None }.size_limit();
    encoded_size_within(width, height, max_width, max_height)
}

/// BGRA pixels of `frame` as a `w`x`h` picture: cropped by a pixel for odd sizes, scaled
/// (nearest neighbour) when `encoded_size` shrank it. A BGRA frame of the same size is borrowed.
fn fit_to_bgra(frame: &RawFrame, w: u32, h: u32) -> Cow<'_, [u8]> {
    let same_size = (w, h) == (frame.width, frame.height);
    let is_bgra = matches!(frame.format, PixelFormat::Bgra);
    if same_size && is_bgra {
        return Cow::Borrowed(&frame.data[..]);
    }
    if same_size {
        return Cow::Owned(frame.to_bgra());
    }
    // Only an RGBA frame needs its channels swapped; BGRA rows are read in place.
    let bgra: Cow<'_, [u8]> = if is_bgra { Cow::Borrowed(&frame.data[..]) } else { Cow::Owned(frame.to_bgra()) };
    let (src_w, src_h) = (frame.width as usize, frame.height as usize);
    let (w, h) = (w as usize, h as usize);
    // Same size class (odd crop): copy rows. Otherwise map each output pixel to a source pixel.
    if w <= src_w && h <= src_h && src_w - w <= 1 && src_h - h <= 1 {
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            out.extend_from_slice(&bgra[y * src_w * 4..(y * src_w + w) * 4]);
        }
        return Cow::Owned(out);
    }
    let x_map: Vec<usize> = (0..w).map(|x| (x * src_w / w) * 4).collect();
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &bgra[(y * src_h / h) * src_w * 4..][..src_w * 4];
        for &sx in &x_map {
            out.extend_from_slice(&row[sx..sx + 4]);
        }
    }
    Cow::Owned(out)
}

/// Whether an encoder that re-sends the last frame should do so now: a key frame was requested,
/// or nothing was sent for a second (keeps a still screen visible and viewers in sync).
pub fn resend_last(key_requested: bool, since_last_sent: Duration) -> bool {
    key_requested || since_last_sent >= Duration::from_secs(1)
}

/// Whether a share running above 30 FPS should drop to 30: encoding is too slow for the target
/// over a window of at least 3 seconds.
fn should_fall_back(target_fps: u8, average_encode: Duration, window: Duration) -> bool {
    target_fps > 30 && window >= Duration::from_secs(3) && average_encode > Duration::from_millis(15)
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
/// What a frame source returned this time.
pub enum FramePoll {
    Frame(RawFrame),
    /// Nothing new since the last call (a still screen); not a failure.
    Idle,
    Failed(String),
}

pub trait FrameSource: Send + 'static {
    fn next_frame(&mut self) -> Result<RawFrame, String>;

    /// Like `next_frame`, but a source may report that nothing changed since the last call.
    fn poll_frame(&mut self) -> FramePoll {
        match self.next_frame() {
            Ok(frame) => FramePoll::Frame(frame),
            Err(e) => FramePoll::Failed(e),
        }
    }
}

/// What a running share reports every two seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShareStats {
    pub captured_fps: f32,
    pub sent_fps: f32,
    /// Average time to fit, convert and encode one frame.
    pub encode_ms: f32,
    pub bitrate_kbps: u32,
    /// The frame rate the share is paced to now (30 after a fallback from 60).
    pub target_fps: u8,
}

/// Optional callbacks of a share; both run on the encoder thread and must return quickly.
#[derive(Default)]
pub struct ShareHooks {
    pub on_stats: Option<Box<dyn Fn(ShareStats) + Send>>,
    pub on_fps_fallback: Option<Box<dyn Fn(u8) + Send>>,
}

/// Counts of one reporting window.
#[derive(Debug, Default)]
struct StatsWindow {
    sent: u32,
    encode_total: Duration,
}

impl StatsWindow {
    fn record_sent(&mut self, encode_time: Duration) {
        self.sent += 1;
        self.encode_total += encode_time;
    }

    /// Turn the window into stats and start a new window.
    fn take(&mut self, captured: u32, elapsed: Duration, bitrate_bps: Option<u32>, target_fps: u8) -> ShareStats {
        let secs = elapsed.as_secs_f32();
        let per_second = |count: u32| if secs > 0.0 { count as f32 / secs } else { 0.0 };
        let encode_ms = if self.sent > 0 {
            self.encode_total.as_secs_f32() * 1000.0 / self.sent as f32
        } else {
            0.0
        };
        let stats = ShareStats {
            captured_fps: per_second(captured),
            sent_fps: per_second(self.sent),
            encode_ms,
            bitrate_kbps: bitrate_bps.map_or(0, |bps| bps / 1000),
            target_fps,
        };
        *self = Self::default();
        stats
    }
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

/// Captures a monitor or a window through a Windows Graphics Capture session: frames arrive as
/// the picture changes, so a still screen (or a minimized window) is idle, not failing.
#[cfg(windows)]
pub struct WgcSource(finch_screen::WgcCapture);

#[cfg(windows)]
impl WgcSource {
    /// How long the first frame may take before the session counts as not working.
    const FIRST_FRAME: Duration = Duration::from_secs(2);
    /// How long one poll waits for a new frame.
    const POLL: Duration = Duration::from_millis(100);

    pub fn monitor(id: u32) -> Result<Self, String> {
        finch_screen::WgcCapture::start(finch_screen::WgcTarget::Monitor(id)).map(Self).map_err(|e| e.to_string())
    }

    pub fn window(id: u32) -> Result<Self, String> {
        finch_screen::WgcCapture::start(finch_screen::WgcTarget::Window(id)).map(Self).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
impl FrameSource for WgcSource {
    fn next_frame(&mut self) -> Result<RawFrame, String> {
        self.0.take_frame(Self::FIRST_FRAME).ok_or_else(|| "Windows Graphics Capture delivered no picture".to_string())
    }

    fn poll_frame(&mut self) -> FramePoll {
        match self.0.take_frame(Self::POLL) {
            Some(frame) => FramePoll::Frame(frame),
            None if self.0.is_closed() => FramePoll::Failed("the shared screen or window is gone".to_string()),
            None => FramePoll::Idle,
        }
    }
}

/// Consecutive capture failures (window closed, display off) before the share ends. A shared window that
/// is minimized yields no frames, so this is about 30 seconds (at 33 ms per try): enough to restore it.
const MAX_CAPTURE_FAILURES: u32 = 900;

/// The newest captured frame, handed from the capture thread to the encoder thread.
#[derive(Default)]
struct FrameSlot {
    frame: Option<RawFrame>,
    /// Set by the capture thread when it gave up.
    failed: Option<String>,
}

type SharedSlot = Arc<(Mutex<FrameSlot>, Condvar)>;

/// A running screen share: one thread captures, another encodes and sends.
///
/// Capture and encoding overlap, and the encoder always takes the newest frame, so a slow
/// encoder drops frames instead of falling behind.
pub struct ScreenSharer {
    stop: Arc<AtomicBool>,
    keyframe: Arc<AtomicBool>,
    loss: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl ScreenSharer {
    /// Start sharing on `connection`. `on_end` runs (on the encoder thread) with the reason if
    /// the share ends by itself - capture keeps failing, the connection is gone - but not after
    /// [`stop`](Self::stop).
    pub fn start(
        connection: quinn::Connection,
        runtime: tokio::runtime::Handle,
        channel_id: u32,
        sender_peer_id: u32,
        config: ShareConfig,
        source: Box<dyn FrameSource>,
        on_end: impl FnOnce(String) + Send + 'static,
    ) -> Self {
        Self::start_with_hooks(
            connection,
            runtime,
            channel_id,
            sender_peer_id,
            config,
            source,
            ShareHooks::default(),
            on_end,
        )
    }

    /// [`start`](Self::start) with callbacks for statistics and the 60 to 30 FPS fallback.
    #[allow(clippy::too_many_arguments)]
    pub fn start_with_hooks(
        connection: quinn::Connection,
        runtime: tokio::runtime::Handle,
        channel_id: u32,
        sender_peer_id: u32,
        config: ShareConfig,
        mut source: Box<dyn FrameSource>,
        hooks: ShareHooks,
        on_end: impl FnOnce(String) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let keyframe = Arc::new(AtomicBool::new(true));
        let loss = Arc::new(AtomicBool::new(false));
        let slot: SharedSlot = Arc::new((Mutex::new(FrameSlot::default()), Condvar::new()));
        let mut threads = Vec::new();
        // Frames captured so far (counted by the capture thread, read by the encoder for stats).
        let captured = Arc::new(AtomicU32::new(0));

        // Capture thread: as fast as the source delivers, keeping only the newest frame.
        let (capture_stop, capture_slot, capture_count) = (stop.clone(), slot.clone(), captured.clone());
        threads.extend(
            std::thread::Builder::new()
                .name("screen-capture".into())
                .spawn(move || {
                    let mut failures = 0u32;
                    while !capture_stop.load(Ordering::Relaxed) {
                        match source.poll_frame() {
                            FramePoll::Idle => {
                                // Nothing new (a still screen): the source has waited already, and this is no failure.
                            }
                            FramePoll::Frame(frame) => {
                                failures = 0;
                                capture_count.fetch_add(1, Ordering::Relaxed);
                                if let Ok(mut guard) = capture_slot.0.lock() {
                                    guard.frame = Some(frame);
                                }
                                capture_slot.1.notify_one();
                            }
                            FramePoll::Failed(e) => {
                                failures += 1;
                                if failures >= MAX_CAPTURE_FAILURES {
                                    if let Ok(mut guard) = capture_slot.0.lock() {
                                        guard.failed = Some(format!("screen capture keeps failing: {e}"));
                                    }
                                    capture_slot.1.notify_one();
                                    return;
                                }
                                std::thread::sleep(Duration::from_millis(33));
                            }
                        }
                    }
                })
                .ok(),
        );

        // Encoder thread: paced to the frame rate.
        let (stop_flag, key_flag, loss_flag) = (stop.clone(), keyframe.clone(), loss.clone());
        threads.extend(
            std::thread::Builder::new()
                .name("screen-encode".into())
                .spawn(move || {
                    let mut packetizer = ScreenPacketizer::new(config, channel_id, sender_peer_id);
                    let mut target_fps = config.fps.max(1);
                    let mut interval = Duration::from_secs_f64(1.0 / f64::from(target_fps));
                    let started = Instant::now();
                    // Pacing by absolute deadlines, so the work of one tick does not shift the next.
                    let mut next = Instant::now();
                    let mut last_frame: Option<RawFrame> = None;
                    let mut last_sent = Instant::now();
                    let mut stats = StatsWindow::default();
                    let mut stats_since = Instant::now();
                    let mut log_since = Instant::now();
                    // Encode times for the 60 to 30 FPS fallback (its own 3 second window).
                    let mut fallback_total = Duration::ZERO;
                    let mut fallback_frames = 0u32;
                    let mut fallback_since = Instant::now();
                    let reason = loop {
                        if stop_flag.load(Ordering::Relaxed) {
                            return;
                        }
                        // Take the newest frame without blocking. Only before the first frame does the
                        // loop wait for one.
                        let fresh = {
                            let Ok(mut guard) = slot.0.lock() else {
                                break "internal error".to_string();
                            };
                            if last_frame.is_none() {
                                while guard.frame.is_none() && guard.failed.is_none() {
                                    if stop_flag.load(Ordering::Relaxed) {
                                        return;
                                    }
                                    guard = match slot.1.wait_timeout(guard, Duration::from_millis(100)) {
                                        Ok((guard, _)) => guard,
                                        Err(_) => return,
                                    };
                                }
                            }
                            match guard.frame.take() {
                                Some(frame) => Some(frame),
                                None => match guard.failed.take() {
                                    Some(reason) => break reason,
                                    None => None,
                                },
                            }
                        };
                        let key_requested = key_flag.swap(false, Ordering::Relaxed);
                        let send = fresh.is_some() || resend_last(key_requested, last_sent.elapsed());
                        if let Some(frame) = fresh {
                            last_frame = Some(frame);
                        }
                        let to_send = if send { last_frame.as_ref() } else { None };
                        if let Some(frame) = to_send {
                            if key_requested {
                                packetizer.request_keyframe();
                            }
                            let Some(max_datagram) = connection.max_datagram_size() else {
                                break "the connection does not support datagrams".to_string();
                            };
                            let timestamp = started.elapsed().as_millis() as u32;
                            if loss_flag.swap(false, Ordering::Relaxed) {
                                packetizer.report_loss(timestamp);
                            }
                            let encode_start = Instant::now();
                            let outcome = packetizer.push_frame(frame, timestamp, max_datagram);
                            let encode_time = encode_start.elapsed();
                            fallback_total += encode_time;
                            fallback_frames += 1;
                            match outcome {
                                Ok(OutgoingVideo::KeyFrame { header, data }) => {
                                    stats.record_sent(encode_time);
                                    last_sent = Instant::now();
                                    let connection = connection.clone();
                                    runtime.spawn(async move {
                                        let _ = crate::session::send_keyframe_on(&connection, &header, &data).await;
                                    });
                                }
                                Ok(OutgoingVideo::Delta(datagrams)) => {
                                    stats.record_sent(encode_time);
                                    last_sent = Instant::now();
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
                        if connection.close_reason().is_some() {
                            break "connection closed".to_string();
                        }

                        // Fallback: a 60 FPS share whose encoder cannot keep up drops to 30 for good.
                        let fallback_elapsed = fallback_since.elapsed();
                        if fallback_elapsed >= Duration::from_secs(3) {
                            let average = if fallback_frames > 0 {
                                fallback_total / fallback_frames
                            } else {
                                Duration::ZERO
                            };
                            if should_fall_back(target_fps, average, fallback_elapsed) {
                                target_fps = 30;
                                interval = Duration::from_secs_f64(1.0 / f64::from(target_fps));
                                packetizer.set_fps(target_fps);
                                tracing::warn!(
                                    "screen share: encoding takes {:.1} ms per frame, falling back to 30 fps",
                                    average.as_secs_f32() * 1000.0
                                );
                                if let Some(on_fallback) = &hooks.on_fps_fallback {
                                    on_fallback(target_fps);
                                }
                            }
                            fallback_total = Duration::ZERO;
                            fallback_frames = 0;
                            fallback_since = Instant::now();
                        }

                        // Statistics every two seconds; a log line every ten.
                        let stats_elapsed = stats_since.elapsed();
                        if stats_elapsed >= Duration::from_secs(2) {
                            stats_since = Instant::now();
                            let captured_now = captured.swap(0, Ordering::Relaxed);
                            let snapshot = stats.take(captured_now, stats_elapsed, packetizer.bitrate_bps(), target_fps);
                            if let Some(on_stats) = &hooks.on_stats {
                                on_stats(snapshot);
                            }
                            if log_since.elapsed() >= Duration::from_secs(10) {
                                log_since = Instant::now();
                                tracing::info!(
                                    "screen share: captured {:.0} fps, sent {:.0} fps, encode {:.1} ms, {} kbit/s",
                                    snapshot.captured_fps,
                                    snapshot.sent_fps,
                                    snapshot.encode_ms,
                                    snapshot.bitrate_kbps
                                );
                            }
                        }

                        next += interval;
                        let now = Instant::now();
                        if next < now {
                            next = now;
                        }
                        std::thread::sleep(next.saturating_duration_since(now));
                    };
                    // Tell the capture thread to stop too, then report.
                    let ended_by_stop = stop_flag.swap(true, Ordering::Relaxed);
                    if !ended_by_stop {
                        on_end(reason);
                    }
                })
                .ok(),
        );

        Self { stop, keyframe, loss, threads }
    }

    /// A viewer lost part of the stream and needs a key frame. Repeated requests also lower
    /// the bitrate.
    pub fn request_keyframe(&self) {
        self.keyframe.store(true, Ordering::Relaxed);
        self.loss.store(true, Ordering::Relaxed);
    }

    /// True while the share is active; false once it has ended or been stopped.
    pub fn is_running(&self) -> bool {
        !self.stop.load(Ordering::Relaxed) && self.threads.iter().all(|t| !t.is_finished())
    }

    /// Stop sharing and wait for both threads.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for ScreenSharer {
    fn drop(&mut self) {
        self.stop();
    }
}


// ---- screen sound ---------------------------------------------------------------------------

/// What a source of screen-sound packets reports when polled.
pub enum AudioPoll {
    Packet(MediaPacket),
    /// Nothing yet.
    Idle,
    /// The source ended.
    Closed,
}

/// Where encoded `ScreenAudio` packets come from: the loopback capturer, or a test channel.
pub trait AudioPacketSource {
    fn poll(&mut self, timeout: Duration) -> AudioPoll;
}

impl AudioPacketSource for std::sync::mpsc::Receiver<MediaPacket> {
    fn poll(&mut self, timeout: Duration) -> AudioPoll {
        match self.recv_timeout(timeout) {
            Ok(packet) => AudioPoll::Packet(packet),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => AudioPoll::Idle,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => AudioPoll::Closed,
        }
    }
}

impl AudioPacketSource for finch_screen::ScreenAudioCapturer {
    fn poll(&mut self, timeout: Duration) -> AudioPoll {
        match self.receiver().recv_timeout(timeout) {
            Ok(packet) => AudioPoll::Packet(packet),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => AudioPoll::Idle,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => AudioPoll::Closed,
        }
    }
}

/// Sends the sound of a shared screen to the viewers as datagrams, until stopped.
pub struct ScreenAudioForwarder {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ScreenAudioForwarder {
    /// Start forwarding. `make_source` runs on the new thread (the loopback capturer must live
    /// on the thread that created it); its error is returned here.
    pub fn start(
        connection: quinn::Connection,
        make_source: impl FnOnce() -> Result<Box<dyn AudioPacketSource>, String> + Send + 'static,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let thread = std::thread::Builder::new()
            .name("screen-audio-send".into())
            .spawn(move || {
                let mut source = match make_source() {
                    Ok(source) => {
                        let _ = ready_tx.send(Ok(()));
                        source
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while !stop_flag.load(Ordering::Relaxed) {
                    match source.poll(Duration::from_millis(50)) {
                        AudioPoll::Packet(packet) => {
                            // A dropped packet is a short glitch; the stream carries on.
                            let _ = connection.send_datagram(packet.encode());
                        }
                        AudioPoll::Idle => {}
                        AudioPoll::Closed => return,
                    }
                    if connection.close_reason().is_some() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self { stop, thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err("screen sound capture did not start in time".into())
            }
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ScreenAudioForwarder {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> RawFrame {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        RawFrame::new(width, height, PixelFormat::Bgra, Bytes::from(data))
    }

    fn filled(width: u32, height: u32, format: PixelFormat, px: [u8; 4]) -> RawFrame {
        let data: Vec<u8> = px.iter().copied().cycle().take((width * height * 4) as usize).collect();
        RawFrame::new(width, height, format, Bytes::from(data))
    }

    #[test]
    fn encoded_size_keeps_the_shape_within_the_limits() {
        assert_eq!(share_size(2560, 1440, 60), (1920, 1080));
        let (w, h) = share_size(3440, 1440, 60);
        assert_eq!(w, 1920);
        assert!(h % 2 == 0 && h <= 1080);
        assert!((f64::from(w) / f64::from(h) - 3440.0 / 1440.0).abs() < 0.01);
        assert_eq!(share_size(1920, 1080, 60), (1920, 1080));
        assert_eq!(share_size(2560, 1440, 30), (2560, 1440));
        assert_eq!(share_size(1921, 1081, 30), (1920, 1080));
        assert!(encoded_size_within(10_000, 2, 1920, 1080).1 >= 2);
    }

    #[test]
    fn size_limit_depends_on_the_frame_rate() {
        assert_eq!(ShareConfig { fps: 60, bitrate_bps: None }.size_limit(), (MAX_WIDTH_60, MAX_HEIGHT_60));
        assert_eq!(ShareConfig { fps: 30, bitrate_bps: None }.size_limit(), (MAX_WIDTH, MAX_HEIGHT));
    }

    #[test]
    fn bitrate_follows_the_picture_and_the_frame_rate() {
        assert_eq!(default_bitrate(1920, 1080, 30), 5_184_000);
        assert_eq!(default_bitrate(1920, 1080, 60), 6_912_000);
        assert_eq!(default_bitrate(3840, 2160, 30), 8_000_000);
        assert_eq!(default_bitrate(640, 360, 60), 1_000_000);
    }

    #[test]
    fn bgra_frame_of_the_same_size_is_borrowed() {
        let frame = filled(8, 8, PixelFormat::Bgra, [1, 2, 3, 255]);
        let out = fit_to_bgra(&frame, 8, 8);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(&out[..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn rgba_frame_of_the_same_size_is_swapped_to_bgra() {
        let frame = filled(8, 8, PixelFormat::Rgba, [200, 100, 50, 255]);
        let out = fit_to_bgra(&frame, 8, 8);
        assert!(matches!(out, Cow::Owned(_)));
        assert_eq!(&out[..4], &[50, 100, 200, 255]);
    }

    #[test]
    fn odd_size_is_cropped_to_the_even_size() {
        let frame = gradient(9, 5);
        let out = fit_to_bgra(&frame, 8, 4);
        assert_eq!(out.len(), 8 * 4 * 4);
        assert_eq!(&out[..4], &[0, 0, 0, 255]);
        assert_eq!(&out[(3 * 8 + 7) * 4..][..4], &[7, 3, 0, 255]);
    }

    #[test]
    fn downscaling_by_two_takes_every_second_pixel() {
        let frame = gradient(8, 8);
        let out = fit_to_bgra(&frame, 4, 4);
        assert_eq!(out.len(), 4 * 4 * 4);
        assert_eq!(&out[..4], &[0, 0, 0, 255]);
        assert_eq!(&out[(4 + 1) * 4..][..4], &[2, 2, 0, 255]);
    }

    #[test]
    fn resend_only_when_asked_or_after_a_second() {
        assert!(!resend_last(false, Duration::from_millis(500)));
        assert!(resend_last(true, Duration::ZERO));
        assert!(resend_last(false, Duration::from_secs(1)));
    }

    #[test]
    fn falls_back_only_for_a_slow_60_fps_share_over_three_seconds() {
        let slow = Duration::from_millis(16);
        let window = Duration::from_secs(3);
        assert!(should_fall_back(60, slow, window));
        assert!(!should_fall_back(30, slow, window), "already at 30");
        assert!(!should_fall_back(60, Duration::from_millis(15), window), "not slower than 15 ms");
        assert!(!should_fall_back(60, slow, Duration::from_secs(2)), "window too short");
    }

    #[test]
    fn stats_count_sent_frames_per_second() {
        let mut window = StatsWindow::default();
        for _ in 0..60 {
            window.record_sent(Duration::from_millis(5));
        }
        let stats = window.take(60, Duration::from_secs(2), Some(4_000_000), 30);
        assert_eq!(stats.sent_fps, 30.0);
        assert_eq!(stats.captured_fps, 30.0);
        assert!((stats.encode_ms - 5.0).abs() < 0.01);
        assert_eq!(stats.bitrate_kbps, 4_000);
        assert_eq!(stats.target_fps, 30);
    }

    #[test]
    fn stats_without_frames_are_zero_and_the_window_restarts() {
        let mut window = StatsWindow::default();
        let stats = window.take(0, Duration::ZERO, None, 60);
        assert_eq!(stats.encode_ms, 0.0);
        assert_eq!(stats.sent_fps, 0.0);
        assert_eq!(stats.captured_fps, 0.0);
        assert_eq!(stats.bitrate_kbps, 0);
        assert_eq!(window.take(0, Duration::from_secs(1), None, 60).sent_fps, 0.0);
    }

    #[test]
    fn a_share_that_falls_back_to_30_keeps_its_size_and_uses_the_30_fps_bitrate() {
        let mut p = ScreenPacketizer::new(ShareConfig { fps: 60, bitrate_bps: None }, 1, 2);
        let frame = RawFrame::new(2560, 1440, PixelFormat::Bgra, Bytes::from(vec![0u8; 2560 * 1440 * 4]));
        p.push_frame(&frame, 0, 1200).unwrap();
        assert_eq!(p.dimensions(), Some((1920, 1080)));
        assert_eq!(p.bitrate_bps(), Some(6_912_000));

        p.set_fps(30);
        let out = p.push_frame(&frame, 33, 1200).unwrap();
        assert!(matches!(out, OutgoingVideo::KeyFrame { .. }));
        assert_eq!(p.dimensions(), Some((1920, 1080)));
        assert_eq!(p.bitrate_bps(), Some(5_184_000));
    }
}
