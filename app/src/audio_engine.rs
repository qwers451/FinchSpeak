//! Audio engine managing microphone capture and speaker playback via cpal.
//!
//! All `cpal::Stream` instances and DSP processing (rubato resampling, AEC,
//! noise suppression, VAD, and Opus codec) run in a dedicated background worker thread.
//! The audio hardware callbacks communicate with the worker thread strictly via lock-free
//! SPSC ring buffers (`ringbuf::HeapRb`) without mutex locks or memory allocations.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::Stream;
use finch_audio::{
    open_input_device, open_output_device, AudioMixer, DeviceChoice, InputResampler, OutputResampler, ReferenceFeeder, VadDecision, VoiceCapture,
    VoiceCaptureConfig, FRAME_SIZE_48K,
};
use finch_audio::quality::QualityTracker;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use tauri::Emitter;
use tracing::{error, info, warn};

/// More restarts than this inside `RESTART_WINDOW` means the devices keep failing: stop trying.
const MAX_DEVICE_RESTARTS: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(10);

use crate::state::AppState;
use crate::audio_diagnostics::{DeviceConfig, Metric};
use crate::capture_privacy::{CaptureClock, CapturedSample, InputBoundary};

/// Capacity of lock-free ring buffers (16384 samples ~ 341 ms at 48 kHz).
const RING_BUFFER_CAPACITY: usize = 16384;

#[derive(Clone, Copy, Default)]
struct PlaybackSample {
    audible: f32,
    echo_reference: f32,
}

// Only continuous live monitoring is excluded. Recorded microphone tests remain
// in the reference alongside remote audio: they can echo while capture is private.
fn playback_sample(reference_source: f32, live_monitor: f32) -> PlaybackSample {
    let combined = reference_source + live_monitor;
    let scale = combined.abs().max(1.0);
    PlaybackSample { audible: combined / scale, echo_reference: reference_source / scale }
}

struct PlaybackResampler {
    audible: OutputResampler,
    reference: OutputResampler,
    rate: u32,
}
impl PlaybackResampler {
    fn new(rate: u32) -> Result<Self, String> {
        Ok(Self { audible: OutputResampler::new(rate).map_err(|e| e.to_string())?,
            reference: OutputResampler::new(rate).map_err(|e| e.to_string())?, rate })
    }
    fn passthrough() -> Self {
        Self { audible: OutputResampler::Passthrough, reference: OutputResampler::Passthrough, rate: 48_000 }
    }
    fn process(&mut self, source: &[f32; FRAME_SIZE_48K], monitor: &[f32; FRAME_SIZE_48K]) -> Result<Vec<PlaybackSample>, String> {
        let result = self.process_frame(source, monitor);
        if result.is_err() { *self = Self::new(self.rate)?; }
        result
    }
    fn process_frame(&mut self, source: &[f32; FRAME_SIZE_48K], monitor: &[f32; FRAME_SIZE_48K]) -> Result<Vec<PlaybackSample>, String> {
        let mut audible = [0.0; FRAME_SIZE_48K];
        let mut reference = [0.0; FRAME_SIZE_48K];
        for index in 0..FRAME_SIZE_48K {
            let sample = playback_sample(source[index], monitor[index]);
            audible[index] = sample.audible; reference[index] = sample.echo_reference;
        }
        let audible = self.audible.process(&audible).map_err(|e| e.to_string())?;
        let reference = self.reference.process(&reference).map_err(|e| e.to_string())?;
        if audible.len() != reference.len() { return Err("Playout and echo reference length mismatch".into()); }
        Ok(audible.into_iter().zip(reference).map(|(audible, echo_reference)| PlaybackSample { audible, echo_reference }).collect())
    }
}

fn playback_headroom(chunk: usize, callback_frames: usize) -> usize {
    // Keep one processing frame beyond the largest observed hardware request.
    // Small callbacks retain the existing 60 ms target. Bound the target so a
    // complete resampled frame still fits in the ring before the next callback.
    (chunk * 3).max(callback_frames.saturating_add(chunk))
        .min(RING_BUFFER_CAPACITY.saturating_sub(chunk))
}

/// If the playout reference ring is this close to full, the output callback has started dropping samples.
const REFERENCE_OVERFLOW_MARGIN: usize = 256;

/// Errors that can occur in the audio engine.
#[derive(Debug, thiserror::Error)]
pub enum AudioEngineError {
    #[error("Failed to spawn audio worker thread: {0}")]
    ThreadSpawnFailed(#[from] std::io::Error),

    #[error("Audio worker command channel disconnected: {0}")]
    ChannelClosed(String),
}

enum AudioCommand {
    Start {
        channel_id: u32,
        state: AppState,
        app_handle: tauri::AppHandle,
    },
    Stop,
}

pub struct AudioEngine {
    cmd_tx: Sender<AudioCommand>,
    is_running: Arc<AtomicBool>,
}

unsafe impl Send for AudioEngine {}
unsafe impl Sync for AudioEngine {}

impl AudioEngine {
    /// Creates and starts a new audio engine with a background worker thread.
    ///
    /// Returns an error if the OS worker thread could not be spawned.
    pub fn try_new() -> Result<Self, AudioEngineError> {
        let (cmd_tx, cmd_rx) = channel::<AudioCommand>();
        let is_running = Arc::new(AtomicBool::new(false));
        let is_running_thread = is_running.clone();

        std::thread::Builder::new()
            .name("finch-audio-worker".into())
            .spawn(move || {
                run_worker(cmd_rx, is_running_thread);
            })
            .map_err(AudioEngineError::ThreadSpawnFailed)?;

        Ok(Self { cmd_tx, is_running })
    }

    /// Creates a new audio engine. If thread spawning fails, logs an error
    /// and returns an inactive engine instance without panicking.
    pub fn new() -> Self {
        match Self::try_new() {
            Ok(engine) => engine,
            Err(err) => {
                error!("Failed to initialize audio engine: {}", err);
                let (cmd_tx, _) = channel::<AudioCommand>();
                Self {
                    cmd_tx,
                    is_running: Arc::new(AtomicBool::new(false)),
                }
            }
        }
    }

    /// Returns whether the audio streams are currently active.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Starts audio capture and playback for the specified voice channel.
    pub fn start(
        &mut self,
        channel_id: u32,
        state: AppState,
        app_handle: tauri::AppHandle,
    ) -> Result<(), AudioEngineError> {
        self.cmd_tx
            .send(AudioCommand::Start {
                channel_id,
                state,
                app_handle,
            })
            .map_err(|e| AudioEngineError::ChannelClosed(e.to_string()))?;
        Ok(())
    }

    /// Stops audio capture and playback streams.
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        let _ = self.cmd_tx.send(AudioCommand::Stop);
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        let _ = self.cmd_tx.send(AudioCommand::Stop);
    }
}

fn capture_config(channel_id: u32, settings: &crate::state::VoiceSettings) -> VoiceCaptureConfig {
    VoiceCaptureConfig { channel_short_id: channel_id, voice_enhancement: settings.voice_enhancement(), vad_threshold_db: settings.vad_threshold_db(), ..Default::default() }
}

fn effective_monitor(settings: &crate::state::VoiceSettings, mic_test_active: bool) -> bool {
    settings.monitor() && !mic_test_active
}
#[derive(Default)]
struct ProcessingFailures { consecutive: u8, reported: bool }
impl ProcessingFailures {
    fn failed(&mut self) -> bool {
        if self.reported { return false; }
        self.consecutive += 1;
        if self.consecutive >= 3 { self.reported = true; true } else { false }
    }
    fn succeeded(&mut self) { if !self.reported { self.consecutive = 0; } }
}

/// The meter is updated every second frame: 25 times a second.
const MIC_LEVEL_EVERY_FRAMES: u32 = 2;

/// "Test my microphone": a few seconds of processed microphone are recorded, then played back.
struct MicTest {
    wanted_frames: usize,
    frames: Vec<[f32; FRAME_SIZE_48K]>,
    played: usize,
    preroll: VecDeque<(usize, [f32; FRAME_SIZE_48K])>,
}

impl MicTest {
    fn new(seconds: u32) -> Self {
        let wanted_frames = (seconds.clamp(1, 10) as usize) * 50;
        Self { wanted_frames, frames: Vec::with_capacity(wanted_frames), played: 0, preroll: VecDeque::with_capacity(finch_audio::PREROLL_FRAMES) }
    }

    /// Keep a private onset buffer for the local preview; the network encoder never sees it.
    fn record(&mut self, frame: &[f32; FRAME_SIZE_48K], active: bool, ptt_open: bool) {
        if self.recording_done() { return; }
        let active = active && ptt_open;
        let index = self.frames.len();
        self.frames.push(if active { *frame } else { [0.0; FRAME_SIZE_48K] });
        if !ptt_open {
            self.preroll.clear();
        } else if active {
            for (old_index, old_frame) in self.preroll.drain(..) {
                self.frames[old_index] = old_frame;
            }
        } else {
            if self.preroll.len() == finch_audio::PREROLL_FRAMES { self.preroll.pop_front(); }
            self.preroll.push_back((index, *frame));
        }
    }

    fn recording_done(&self) -> bool {
        self.frames.len() >= self.wanted_frames
    }

    /// The next recorded frame to play, once recording has finished.
    fn next_playback_frame(&mut self) -> Option<[f32; FRAME_SIZE_48K]> {
        if !self.recording_done() {
            return None;
        }
        let frame = self.frames.get(self.played).copied();
        self.played += 1;
        frame
    }

    fn finished(&self) -> bool {
        self.recording_done() && self.played >= self.frames.len()
    }
}

/// Mix the next 20 ms playback frame.
///
/// Runs on the audio worker thread (a plain OS thread, not a cpal callback and not inside
/// the Tokio runtime), so it may wait briefly while the network task pushes a packet;
/// skipping the frame instead would insert 20 ms of silence (a click).
fn mix_next_frame(
    mixer: &tokio::sync::Mutex<AudioMixer>,
    frame: &mut [f32; FRAME_SIZE_48K],
    now: Instant,
) -> Vec<(u32, bool)> {
    mixer.blocking_lock().mix_frame(frame, now)
}

/// Render paired samples without allowing local monitoring into the AEC reference.
fn render_playback(
    data: &mut [f32], channels: usize,
    playback: &mut impl Consumer<Item = PlaybackSample>,
    reference: &mut impl Producer<Item = f32>,
) -> (u64, u64) {
    let mut missing = 0;
    let mut reference_dropped = 0;
    for chunk in data.chunks_mut(channels) {
        let sample = playback.try_pop().unwrap_or_else(|| { missing += 1; PlaybackSample::default() });
        if reference.try_push(sample.echo_reference).is_err() { reference_dropped += 1; }
        chunk.fill(sample.audible);
    }
    (missing, reference_dropped)
}

/// Discard queued playout while deafened; only actual output silence goes to AEC.
fn render_deafened(
    data: &mut [f32], channels: usize,
    playback: &mut impl Consumer<Item = PlaybackSample>,
    reference: &mut impl Producer<Item = f32>,
) {
    let queued = playback.occupied_len();
    playback.skip(queued);
    data.fill(0.0);
    for _ in 0..data.len() / channels {
        let _ = reference.try_push(0.0);
    }
}

/// Tell the UI when the chosen device was not available and the default is used instead.
fn report_fallback(kind: &str, choice: &DeviceChoice, app_handle: &tauri::AppHandle) {
    if let DeviceChoice::FellBack { wanted } = choice {
        warn!("Chosen {} device {:?} is not available; using the default", kind, wanted);
        let _ = app_handle.emit(
            "audio-device-fallback",
            serde_json::json!({ "kind": kind, "wanted": wanted }),
        );
    }
}

/// Dedicated OS thread function managing cpal streams, DSP, and lock-free ring buffers.
fn run_worker(cmd_rx: Receiver<AudioCommand>, is_running: Arc<AtomicBool>) {
    let mut pending_cmd: Option<AudioCommand> = None;
    let mut restart_times: std::collections::VecDeque<Instant> = std::collections::VecDeque::new();

    loop {
        let cmd = match pending_cmd.take() {
            Some(c) => c,
            None => match cmd_rx.recv() {
                Ok(c) => c,
                Err(_) => {
                    // AudioEngine dropped, terminate thread
                    is_running.store(false, Ordering::SeqCst);
                    break;
                }
            },
        };

        match cmd {
            AudioCommand::Start {
                channel_id,
                state,
                app_handle,
            } => {
                is_running.store(true, Ordering::SeqCst);
                info!("Starting audio worker streams for channel {}", channel_id);

                let selection = state.audio_devices.read().map(|s| s.clone()).unwrap_or_default();
                let diagnostics = state.audio_diagnostics.clone();
                diagnostics.stream_begin();
                // Set from cpal's error callbacks when a stream dies (e.g. the headset is unplugged).
                let stream_failed = Arc::new(AtomicBool::new(false));

                // 1. Output (Playback) Ring Buffer & cpal Stream
                let playback_rb = HeapRb::<PlaybackSample>::new(RING_BUFFER_CAPACITY);
                let (mut playback_prod, mut playback_cons) = playback_rb.split();

                // What the speakers actually played, for echo cancellation: the output callback
                // copies every sample it plays into this lock-free ring.
                let reference_rb = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
                let (mut reference_prod, mut reference_cons) = reference_rb.split();
                let mut reference_feeder: Option<ReferenceFeeder> = None;
                // How long the output device holds audio before it plays (set by the callback).
                let output_latency_ms = Arc::new(AtomicU32::new(0));
                let mut applied_latency_ms = 0u32;

                let mut playback_stream: Option<Stream> = None;
                let mut output_resampler: Option<PlaybackResampler> = None;
                let mut output_sample_rate = 48000u32;
                let output_callback_frames = Arc::new(AtomicU32::new(0));

                let (output_device, output_choice) = open_output_device(selection.output.as_deref());
                report_fallback("output", &output_choice, &app_handle);
                if let Some(device) = output_device {
                    match device.default_output_config() {
                        Ok(config) => {
                            info!(device = ?device.name(), rate = config.sample_rate().0,
                                channels = config.channels(), format = ?config.sample_format(),
                                "Audio output configuration");
                            output_sample_rate = config.sample_rate().0;
                            diagnostics.configure(false, DeviceConfig { name: device.name().unwrap_or_default(),
                                sample_rate: output_sample_rate, channels: config.channels(), sample_format: format!("{:?}", config.sample_format()) });
                            reference_feeder = match ReferenceFeeder::new(output_sample_rate) {
                                Ok(feeder) => Some(feeder),
                                Err(e) => {
                                    error!("Echo cancellation reference disabled: {}", e);
                                    None
                                }
                            };
                            let channels = config.channels() as usize;
                            let is_deafened_ref = state.is_deafened.clone();
                            let latency_ref = output_latency_ms.clone();
                            let callback_frames_ref = output_callback_frames.clone();
                            let output_diagnostics = diagnostics.clone();
                            let mut previous_output_callback: Option<(u64, Instant)> = None;

                            output_resampler = match PlaybackResampler::new(output_sample_rate) {
                                Ok(resampler) => Some(resampler),
                                Err(e) => {
                                    error!("Failed to initialize OutputResampler: {}", e);
                                    Some(PlaybackResampler::passthrough())
                                }
                            };

                            let failed = stream_failed.clone();
                            let err_fn = move |err| {
                                error!("Audio output error: {}", err);
                                failed.store(true, Ordering::SeqCst);
                            };
                            let stream_res = device.build_output_stream(
                                &config.into(),
                                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                                    let timestamp = info.timestamp();
                                    if let Some(latency) = timestamp.playback.duration_since(&timestamp.callback) {
                                        latency_ref.store(latency.as_millis() as u32, Ordering::Relaxed);
                                    }
                                    if channels == 0 {
                                        return;
                                    }
                                    let diagnostic_epoch = output_diagnostics.epoch();
                                    if diagnostic_epoch != 0 {
                                        let now = Instant::now();
                                        let gap = previous_output_callback.filter(|(epoch, _)| *epoch == diagnostic_epoch)
                                            .map(|(_, last)| now.duration_since(last).as_micros() as u64).unwrap_or(0);
                                        previous_output_callback = Some((diagnostic_epoch, now));
                                        output_diagnostics.callback_at(diagnostic_epoch, false, data.len() / channels, gap);
                                    } else { previous_output_callback = None; }
                                    callback_frames_ref.fetch_max(
                                        (data.len() / channels).min(u32::MAX as usize) as u32,
                                        Ordering::Relaxed,
                                    );
                                    if is_deafened_ref.load(Ordering::Relaxed) {
                                        render_deafened(data, channels, &mut playback_cons, &mut reference_prod);
                                        return;
                                    }
                                    let (missing, reference_dropped) = render_playback(data, channels, &mut playback_cons, &mut reference_prod);
                                    output_diagnostics.add_at(diagnostic_epoch, Metric::OutputUnderflowSamples, missing);
                                    output_diagnostics.add_at(diagnostic_epoch, Metric::OutputUnderflowCallbacks, u64::from(missing > 0));
                                    output_diagnostics.add_at(diagnostic_epoch, Metric::ReferenceDroppedSamples, reference_dropped);
                                },
                                err_fn,
                                None,
                            );

                            match stream_res {
                                Ok(stream) => {
                                    if let Err(e) = stream.play() {
                                        error!("Failed to start playback stream: {}", e);
                                    } else {
                                        playback_stream = Some(stream);
                                        diagnostics.stream_active(false, true);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to build playback stream: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to get default output audio config: {}", e);
                        }
                    }
                } else {
                    error!("No default audio output device available");
                }

                // 2. Input (Capture) Ring Buffer & cpal Stream
                let capture_rb = HeapRb::<CapturedSample>::new(RING_BUFFER_CAPACITY);
                let (mut capture_prod, mut capture_cons) = capture_rb.split();

                let mut capture_stream: Option<Stream> = None;
                let mut input_resampler: Option<InputResampler> = None;

                let (input_device, input_choice) = open_input_device(selection.input.as_deref());
                report_fallback("input", &input_choice, &app_handle);
                if let Some(device) = input_device {
                    match device.default_input_config() {
                        Ok(config) => {
                            info!(device = ?device.name(), rate = config.sample_rate().0,
                                channels = config.channels(), format = ?config.sample_format(),
                                "Audio input configuration");
                            let sample_rate = config.sample_rate().0;
                            diagnostics.configure(true, DeviceConfig { name: device.name().unwrap_or_default(),
                                sample_rate, channels: config.channels(), sample_format: format!("{:?}", config.sample_format()) });
                            let channels = config.channels() as usize;

                            input_resampler = match InputResampler::new(sample_rate) {
                                Ok(resampler) => Some(resampler),
                                Err(e) => {
                                    error!("Failed to initialize InputResampler: {}", e);
                                    Some(InputResampler::Passthrough)
                                }
                            };

                            let failed = stream_failed.clone();
                            let err_fn = move |err| {
                                error!("Audio input error: {}", err);
                                failed.store(true, Ordering::SeqCst);
                            };
                            let privacy = state.voice_settings.clone();
                            let mut capture_clock = CaptureClock::default();
                            let input_diagnostics = diagnostics.clone();
                            let mut previous_input_callback: Option<(u64, Instant)> = None;
                            let stream_res = device.build_input_stream(
                                &config.into(),
                                move |data: &[f32], info: &cpal::InputCallbackInfo| {
                                    let (generation, open) = privacy.privacy_snapshot();
                                    let timestamp = info.timestamp();
                                    let tag = capture_clock.tag(generation, open, timestamp.capture, timestamp.callback);
                                    if channels == 0 {
                                        return;
                                    }
                                    let diagnostic_epoch = input_diagnostics.epoch();
                                    if diagnostic_epoch != 0 {
                                        let now = Instant::now();
                                        let gap = previous_input_callback.filter(|(epoch, _)| *epoch == diagnostic_epoch)
                                            .map(|(_, last)| now.duration_since(last).as_micros() as u64).unwrap_or(0);
                                        previous_input_callback = Some((diagnostic_epoch, now));
                                        input_diagnostics.callback_at(diagnostic_epoch, true, data.len() / channels, gap);
                                    } else { previous_input_callback = None; }
                                    let mut dropped = 0;
                                    for frame in data.chunks(channels) {
                                        let mut sum = 0.0f32;
                                        for &s in frame {
                                            sum += s;
                                        }
                                        let mono = sum / (channels as f32);
                                        if capture_prod.try_push(CapturedSample { sample: mono, generation: tag }).is_err() { dropped += 1; }
                                    }
                                    input_diagnostics.add_at(diagnostic_epoch, Metric::CaptureDroppedSamples, dropped);
                                    input_diagnostics.maximum_at(diagnostic_epoch, Metric::CaptureHighWaterSamples, capture_prod.occupied_len() as u64);
                                },
                                err_fn,
                                None,
                            );

                            match stream_res {
                                Ok(stream) => {
                                    if let Err(e) = stream.play() {
                                        error!("Failed to start capture stream: {}", e);
                                    } else {
                                        capture_stream = Some(stream);
                                        diagnostics.stream_active(true, true);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to build capture stream: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to get default input audio config: {}", e);
                        }
                    }
                } else {
                    error!("No default audio input device available");
                }

                if playback_stream.is_none() && capture_stream.is_none() {
                    diagnostics.tick("startup-failed", state.is_muted.load(Ordering::Relaxed), state.is_deafened.load(Ordering::Relaxed));
                    error!("Both playback and capture audio streams failed to initialize");
                    is_running.store(false, Ordering::SeqCst);
                    continue;
                }

                // 3. DSP pipeline for worker thread processing
                let mut voice_capture = match VoiceCapture::new(capture_config(channel_id, &state.voice_settings)) {
                    Ok(mut capture) => { capture.set_transmit_gate(false); Some(capture) },
                    Err(e) => {
                        is_running.store(false, Ordering::SeqCst);
                        error!("Failed to create microphone processing: {}", e);
                        let _ = app_handle.emit("audio-processing-failed", serde_json::json!({ "message": e.to_string() }));
                        None
                    }
                };

                let mut observed_generation = 0;
                let mut input_boundary = InputBoundary::default();
                let mut capture_tagged_buf = Vec::with_capacity(4096);
                state.voice_settings.set_mic_test(false);
                let mut processing_failures = ProcessingFailures::default();
                let mut is_currently_speaking = false;
                // Meter and link quality for the UI, and the "test my microphone" loop.
                let mut level_frames: u32 = 0;
                let mut quality_tracker = QualityTracker::new();
                let mut last_quality_report = Instant::now();
                let mut mic_test: Option<MicTest> = None;
                // The processed microphone, played back to the user while "hear myself" is on.
                let mut monitor_queue: std::collections::VecDeque<[f32; FRAME_SIZE_48K]> = std::collections::VecDeque::new();
                let mut playback_frame_48k = [0.0f32; FRAME_SIZE_48K];
                let mut monitor_frame_48k = [0.0f32; FRAME_SIZE_48K];
                let mut capture_frame_48k = [0.0f32; FRAME_SIZE_48K];
                let mut capture_raw_buf = Vec::with_capacity(4096);

                let target_output_chunk = ((output_sample_rate as usize * 20) / 1000).max(1);
                let mut previous_playback_refill: Option<(u64, Instant)> = None;

                // 4. Real-time active worker processing loop
                while is_running.load(Ordering::SeqCst) {
                    let diagnostic_mode = match &mic_test {
                        Some(test) if test.recording_done() => "microphone-test-playing",
                        Some(_) => "microphone-test-recording",
                        None if state.voice_settings.monitor() => "monitor",
                        None => "voice",
                    };
                    diagnostics.tick(diagnostic_mode, state.is_muted.load(Ordering::Relaxed), state.is_deafened.load(Ordering::Relaxed));
                    match cmd_rx.try_recv() {
                        Ok(new_cmd) => {
                            pending_cmd = Some(new_cmd);
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            is_running.store(false, Ordering::SeqCst);
                            return;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    }

                    if state.is_muted.load(Ordering::Relaxed) {
                        monitor_queue.clear();
                        if mic_test.take().is_some() {
                            state.voice_settings.set_mic_test(false);
                            let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "done" }));
                        }
                    }
                    // A0. A device stream died (headset unplugged, driver reset): reopen the devices,
                    // falling back to the defaults, instead of leaving the call silent.
                    if stream_failed.swap(false, Ordering::SeqCst) {
                        let now = Instant::now();
                        restart_times.retain(|t| now.duration_since(*t) < RESTART_WINDOW);
                        restart_times.push_back(now);
                        if restart_times.len() > MAX_DEVICE_RESTARTS {
                            error!("Audio devices keep failing; stopping audio");
                            let _ = app_handle.emit("audio-device-failed", ());
                            is_running.store(false, Ordering::SeqCst);
                            break;
                        }
                        warn!("Audio stream failed; reopening the audio devices");
                        let _ = app_handle.emit("audio-device-lost", ());
                        std::thread::sleep(Duration::from_millis(500));
                        pending_cmd = Some(AudioCommand::Start {
                            channel_id,
                            state: state.clone(),
                            app_handle: app_handle.clone(),
                        });
                        break;
                    }

                    // A-. A microphone test was asked for: record a few seconds, then play them back.
                    let requested = state.mic_test_request.swap(0, Ordering::SeqCst);
                    if requested > 0 && mic_test.is_none() {
                        if capture_stream.is_some() && playback_stream.is_some() && !state.is_muted.load(Ordering::Relaxed) {
                            state.voice_settings.set_mic_test(true);
                            monitor_queue.clear();
                            mic_test = Some(MicTest::new(requested));
                            let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "recording", "seconds": requested }));
                        } else {
                            let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "done" }));
                        }
                    }

                    // A-. Once a second: how good is each speaker's link (loss of the last second).
                    if last_quality_report.elapsed() >= Duration::from_secs(1) {
                        last_quality_report = Instant::now();
                        let counters = state.mixer.blocking_lock().loss_counters();
                        let report: Vec<_> = quality_tracker
                            .update(&counters)
                            .into_iter()
                            .map(|(peer_id, quality, loss)| serde_json::json!({ "peer_id": peer_id, "quality": quality.as_str(), "loss": loss }))
                            .collect();
                        let _ = app_handle.emit("voice-quality", report);
                    }

                    // A. Playback processing (mixer -> OutputResampler -> playback_prod)
                    if playback_stream.is_some() {
                        if let Some(resampler) = &mut output_resampler {
                            let target_playback_headroom = playback_headroom(target_output_chunk,
                                output_callback_frames.load(Ordering::Relaxed) as usize);
                            while playback_prod.occupied_len() < target_playback_headroom
                                && playback_prod.vacant_len() >= target_output_chunk
                            {
                                let diagnostic_epoch = diagnostics.epoch();
                                if diagnostic_epoch != 0 {
                                    let now = Instant::now();
                                    if let Some((_, last)) = previous_playback_refill.filter(|(epoch, _)| *epoch == diagnostic_epoch) {
                                        diagnostics.maximum_at(diagnostic_epoch, Metric::PlaybackRefillMaxGapUs, now.duration_since(last).as_micros() as u64);
                                    }
                                    previous_playback_refill = Some((diagnostic_epoch, now));
                                } else { previous_playback_refill = None; }
                                let speech_events =
                                    mix_next_frame(&state.mixer, &mut playback_frame_48k, Instant::now());
                                if let Some(test) = &mut mic_test {
                                    if let Some(recorded) = test.next_playback_frame() {
                                        for (out, mine) in playback_frame_48k.iter_mut().zip(recorded.iter()) {
                                            *out = (*out + *mine).clamp(-1.0, 1.0);
                                        }
                                    }
                                    if test.finished() {
                                        let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "done" }));
                                        mic_test = None;
                                        state.voice_settings.set_mic_test(false);
                                    }
                                }
                                monitor_frame_48k.fill(0.0);
                                if effective_monitor(&state.voice_settings, mic_test.is_some()) {
                                    if let Some(own) = monitor_queue.pop_front() {
                                        monitor_frame_48k = own;
                                    } else {
                                        diagnostics.add(Metric::MonitorEmptyFrames, 1);
                                    }
                                }

                                for (peer_id, speaking) in speech_events {
                                    let _ = app_handle.emit(
                                        "user-speaking",
                                        serde_json::json!({
                                            "peer_id": peer_id,
                                            "is_speaking": speaking
                                        }),
                                    );
                                }

                                match resampler.process(&playback_frame_48k, &monitor_frame_48k) {
                                    Ok(resampled) => {
                                        for &sample in &resampled {
                                            let _ = playback_prod.try_push(sample);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Output resampling error: {}", e);
                                        for _ in 0..target_output_chunk {
                                            let _ = playback_prod.try_push(PlaybackSample::default());
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // A2. Echo cancellation reference: what the speakers actually played.
                    if let (Some(feeder), Some(capture)) = (&mut reference_feeder, &mut voice_capture) {
                        let latency = output_latency_ms.load(Ordering::Relaxed);
                        if latency.abs_diff(applied_latency_ms) >= 10 {
                            if let Err(error) = capture.set_stream_delay_ms(latency) {
                                error!(%error, "Could not apply microphone echo delay");
                            }
                            applied_latency_ms = latency;
                        }
                        if reference_cons.occupied_len() >= RING_BUFFER_CAPACITY - REFERENCE_OVERFLOW_MARGIN {
                            // The callback outran us and dropped samples: the stream is no longer
                            // contiguous, so forget the echo path instead of learning a wrong one.
                            warn!("Playout reference overflow; resetting echo canceller");
                            reference_cons.clear();
                            feeder.clear();
                            if let Err(error) = capture.reset_echo_canceller() {
                                error!(%error, "Could not reset echo canceller");
                                is_running.store(false, Ordering::SeqCst);
                                let _ = app_handle.emit("audio-processing-failed", serde_json::json!({ "message": error.to_string() }));
                                break;
                            }
                        }
                        let mut played = [0.0f32; 1024];
                        loop {
                            let n = reference_cons.pop_slice(&mut played);
                            if n == 0 {
                                break;
                            }
                            feeder.push(&played[..n], |frame| {
                                if let Err(error) = capture.push_playback_reference(frame) {
                                    error!(%error, "Could not process echo reference");
                                }
                            });
                        }
                    }

                    // Epoch changes are observable even if close/open both occurred between polls.
                    let (generation, requested_gate) = state.voice_settings.privacy_snapshot();
                    if observed_generation != generation {
                        observed_generation = generation;
                        capture_cons.clear();
                        capture_raw_buf.clear();
                        if let Some(resampler) = &mut input_resampler { resampler.reset(); }
                        if let Some(capture) = &mut voice_capture { capture.set_transmit_gate(false); }
                    }
                    // B. Capture processing (capture_cons -> InputResampler -> AEC -> Noise -> VAD -> Opus)
                    let batch_epoch = diagnostics.epoch();
                    let batch_started = (batch_epoch != 0).then(Instant::now);
                    let mut batch_frames = 0;
                    if capture_stream.is_some() {
                        if let Some(resampler) = &mut input_resampler {
                            let needed = resampler.input_frames_needed();
                            if needed > 0 {
                                while capture_cons.occupied_len() >= needed {
                                    batch_frames += 1;
                                    capture_raw_buf.clear();
                                    capture_tagged_buf.clear();
                                    for _ in 0..needed {
                                        if let Some(s) = capture_cons.try_pop() {
                                            capture_tagged_buf.push(s);
                                            capture_raw_buf.push(s.sample);
                                        }
                                    }

                                    if capture_raw_buf.len() != needed {
                                        break;
                                    }

                                    if state.is_muted.load(Ordering::Relaxed) {
                                        if let Some(capture) = &mut voice_capture {
                                            capture.set_transmit_gate(false);
                                        }
                                        if is_currently_speaking {
                                            is_currently_speaking = false;
                                            let _ = app_handle.emit("local-speaking", false);
                                        }
                                        continue;
                                    }

                                    let current = state.voice_settings.privacy_snapshot();
                                    let fresh = input_boundary.prepare(&capture_tagged_buf, generation,
                                        requested_gate && current == (generation, true), resampler);
                                    if let Some(capture) = &mut voice_capture {
                                        capture.set_transmit_gate(fresh);
                                    }

                                    if let Err(e) =
                                        resampler.process(&capture_raw_buf, &mut capture_frame_48k)
                                    {
                                        error!("Input resampling error: {}", e);
                                        continue;
                                    }

                                    let Some(capture) = &mut voice_capture else {
                                        continue;
                                    };

                                    let now_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
                                        Ok(dur) => dur.as_millis() as u32,
                                        Err(_) => 0,
                                    };
                                    let sender_id = state.session_peer_id.load(Ordering::Relaxed);
                                    let settings = capture.set_voice_enhancement(state.voice_settings.voice_enhancement())
                                        .and_then(|_| capture.set_input_gain(state.voice_settings.input_gain()));
                                    if let Err(error) = settings {
                                        capture.set_transmit_gate(false);
                                        monitor_queue.clear();
                                        if processing_failures.failed() {
                                            is_running.store(false, Ordering::SeqCst);
                                            let _ = app_handle.emit("audio-processing-failed", serde_json::json!({ "message": error.to_string() }));
                                            break;
                                        }
                                        continue;
                                    }
                                    capture.set_vad_threshold_db(state.voice_settings.vad_threshold_db());
                                    // Recheck after processing/configuration work; opening requires
                                    // a complete chunk proven newer than the device-clock cutoff.
                                    let frame_gate = fresh
                                        && state.voice_settings.privacy_snapshot() == (generation, true);
                                    capture.set_transmit_gate(frame_gate);
                                    let dsp_epoch = diagnostics.epoch();
                                    capture.set_diagnostics_enabled(dsp_epoch != 0);
                                    let dsp_started = (dsp_epoch != 0).then(Instant::now);
                                    let processed = capture.process(&mut capture_frame_48k, sender_id, now_ms);
                                    if let Some(started) = dsp_started { diagnostics.dsp_frame_at(dsp_epoch, started.elapsed().as_micros() as u64); }
                                    let output = match processed {
                                        Ok(output) => output,
                                        Err(error) => {
                                            error!(%error, "Microphone processing failed");
                                            monitor_queue.clear();
                                            if processing_failures.failed() {
                                                is_running.store(false, Ordering::SeqCst);
                                                let _ = app_handle.emit("audio-processing-failed", serde_json::json!({ "message": error.to_string() }));
                                                break;
                                            }
                                            continue;
                                        }
                                    };
                                    processing_failures.succeeded();
                                    if let Some(stages) = output.diagnostics {
                                        diagnostics.signal_frame_at(dsp_epoch, stages.stage_power, stages.gate_changes, stages.dsp_resets);
                                    }
                                    let ptt_open = state.voice_settings.transmit_open()
                                        && !state.is_muted.load(Ordering::Relaxed);
                                    let preview_active = output.activity.should_transmit() && ptt_open;
                                    level_frames += 1;
                                    if level_frames >= MIC_LEVEL_EVERY_FRAMES {
                                        level_frames = 0;
                                        let _ = app_handle.emit(
                                            "mic-level",
                                            serde_json::json!({ "db": output.level_db, "speaking": preview_active }),
                                        );
                                    }
                                    if let Some(test) = &mut mic_test {
                                        if !test.recording_done() {
                                            if !preview_active { diagnostics.add(Metric::TestActivationRejectedFrames, 1); }
                                            if !ptt_open { diagnostics.add(Metric::TestPttClosedFrames, 1); }
                                            test.record(&capture_frame_48k, preview_active, ptt_open);
                                            if test.recording_done() {
                                                let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "playing" }));
                                            }
                                        }
                                    }
                                    if effective_monitor(&state.voice_settings, mic_test.is_some()) {
                                        if monitor_queue.len() >= 5 {
                                            monitor_queue.pop_front();
                                            diagnostics.add(Metric::MonitorDroppedFrames, 1);
                                        }
                                        monitor_queue.push_back(capture_frame_48k);
                                    } else {
                                        monitor_queue.clear();
                                    }

                                    if output.vad == VadDecision::Speaking {
                                        if !is_currently_speaking {
                                            is_currently_speaking = true;
                                            let _ = app_handle.emit("local-speaking", true);
                                        }
                                    } else if output.vad == VadDecision::Silence {
                                        if is_currently_speaking {
                                            is_currently_speaking = false;
                                            let _ = app_handle.emit("local-speaking", false);
                                        }
                                    }

                                    if !output.packets.is_empty()
                                        && state.voice_settings.privacy_snapshot() == (generation, true) {
                                        if let Ok(conn_guard) = state.quic_connection.try_read() {
                                            if let Some(conn) = &*conn_guard {
                                                for packet in output.packets {
                                                    if state.voice_settings.privacy_snapshot() != (generation, true) { break; }
                                                    let _ = conn.send_datagram(packet);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if let Some(started) = batch_started {
                        diagnostics.maximum_at(batch_epoch, Metric::CaptureBatchMaxFrames, batch_frames);
                        diagnostics.maximum_at(batch_epoch, Metric::CaptureBatchMaxUs, started.elapsed().as_micros() as u64);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }

                if is_currently_speaking {
                    let _ = app_handle.emit("local-speaking", false);
                }
                if mic_test.is_some() {
                    state.voice_settings.set_mic_test(false);
                    let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "done" }));
                }

                diagnostics.stream_active(true, false);
                diagnostics.stream_active(false, false);
                diagnostics.tick("stopped", state.is_muted.load(Ordering::Relaxed), state.is_deafened.load(Ordering::Relaxed));
                info!("Audio streams for channel {} stopped", channel_id);
            }
            AudioCommand::Stop => {
                is_running.store(false, Ordering::SeqCst);
                info!("Audio worker stopped");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_render_outputs_monitor_but_references_only_remote_and_test() {
        let (mut producer, mut playback) = HeapRb::<PlaybackSample>::new(16).split();
        let (mut reference, mut received) = HeapRb::<f32>::new(16).split();
        assert!(producer.try_push(playback_sample(0.0, 0.4)).is_ok());
        assert!(producer.try_push(playback_sample(0.2, 0.1)).is_ok());
        let mut stereo = [0.75; 8];
        assert_eq!(render_playback(&mut stereo, 2, &mut playback, &mut reference), (2, 0));
        assert!((stereo[0] - 0.4).abs() < 1e-6);
        assert!((stereo[2] - 0.3).abs() < 1e-6);
        assert_eq!(stereo[0], stereo[1]); assert_eq!(stereo[2], stereo[3]);
        assert_eq!(&stereo[4..], &[0.0; 4]);
        assert_eq!(received.try_pop(), Some(0.0));
        assert_eq!(received.try_pop(), Some(0.2));
        assert_eq!(received.try_pop(), Some(0.0));
        assert_eq!(received.try_pop(), Some(0.0));
    }

    #[test]
    fn local_preview_is_audible_but_never_becomes_far_end_reference() {
        let local = playback_sample(0.0, 0.4);
        assert_eq!(local.audible, 0.4);
        assert_eq!(local.echo_reference, 0.0);
        let remote = playback_sample(0.3, 0.0);
        assert_eq!(remote.audible, 0.3);
        assert_eq!(remote.echo_reference, 0.3);
        let clipped = playback_sample(0.8, 0.8);
        assert_eq!(clipped.audible, 1.0);
        assert_eq!(clipped.echo_reference, 0.5);
    }

    #[test]
    fn monitor_feedback_preserves_full_capture_speech_like_no_monitor() -> anyhow::Result<()> {
        let speech = finch_testbed::audio::load_speech_48k("speech_1")?;
        let far_end = finch_testbed::audio::load_speech_48k("speech_2")?;
        for initial_remote_frames in [0, 100] {
        for enhancement in [false, true] {
        for delay_frames in [3usize, 6] {
            let mut baseline = VoiceCapture::new(VoiceCaptureConfig { voice_enhancement: enhancement, ..Default::default() })?;
            let mut monitored = VoiceCapture::new(VoiceCaptureConfig { voice_enhancement: enhancement, ..Default::default() })?;
            baseline.set_transmit_gate(false); monitored.set_transmit_gate(false);
            baseline.set_stream_delay_ms(10)?; monitored.set_stream_delay_ms(10)?;
            // Activate AEC with real far-end audio first. The test must also work
            // after the room becomes quiet, while has_reference remains latched.
            for index in 0..initial_remote_frames {
                let mut remote = [0.0; FRAME_SIZE_48K];
                for (offset, sample) in remote.iter_mut().enumerate() { *sample = far_end[(index * FRAME_SIZE_48K + offset) % far_end.len()] * 0.1; }
                baseline.push_playback_reference(&remote)?; monitored.push_playback_reference(&remote)?;
                let mut left = remote.map(|sample| sample * 0.3); let mut right = left;
                baseline.process(&mut left, 1, index as u32 * 20)?; monitored.process(&mut right, 1, index as u32 * 20)?;
            }
            let mut references: Vec<[f32; FRAME_SIZE_48K]> = Vec::new();
            let mut max_difference = 0.0f32;
            for index in 0..800 {
                let mut input = [0.0; FRAME_SIZE_48K];
                for (sample_index, sample) in input.iter_mut().enumerate() {
                    *sample = speech[(index * FRAME_SIZE_48K + sample_index) % speech.len()];
                }
                let mut expected = input;
                baseline.push_playback_reference(&[0.0; FRAME_SIZE_48K])?;
                let reference = if index >= delay_frames { references[index - delay_frames] } else { [0.0; FRAME_SIZE_48K] };
                monitored.push_playback_reference(&reference)?;
                baseline.process(&mut expected, 1, index as u32 * 20)?;
                monitored.process(&mut input, 1, index as u32 * 20)?;
                for (&actual, &expected) in input.iter().zip(&expected) { max_difference = max_difference.max((actual - expected).abs()); }
                references.push(input.map(|sample| playback_sample(0.0, sample).echo_reference));
            }
            assert!(max_difference < 1e-6, "local monitor must not alter capture: remote_frames={initial_remote_frames} enhancement={enhancement} lag={delay_frames}, difference={max_difference}");
        }
        }
        }
        Ok(())
    }

    #[test]
    fn paired_resamplers_preserve_remote_and_isolate_monitor_tail() {
        for rate in [16_000, 44_100, 48_000, 96_000] {
            let mut mixed = PlaybackResampler::new(rate).unwrap();
            let mut reference_only = OutputResampler::new(rate).unwrap();
            let mut monitor_only = PlaybackResampler::new(rate).unwrap();
            let mut heard_monitor = false;
            for index in 0..80 {
                let source = std::array::from_fn(|n| (((index * FRAME_SIZE_48K + n) as f32) * 0.03).sin() * 0.2);
                let local = if index < 40 { [0.15; FRAME_SIZE_48K] } else { [0.0; FRAME_SIZE_48K] };
                let pairs = mixed.process(&source, &local).unwrap();
                let expected = reference_only.process(&source).unwrap();
                assert_eq!(pairs.len(), expected.len());
                for (sample, expected) in pairs.iter().zip(&expected) {
                    assert!((sample.echo_reference - expected).abs() < 1e-6, "far-end reference changed at {rate} Hz");
                }
                let own = monitor_only.process(&[0.0; FRAME_SIZE_48K], &local).unwrap();
                assert!(own.iter().all(|sample| sample.echo_reference == 0.0), "local FFT tail leaked into AEC at {rate} Hz");
                heard_monitor |= own.iter().any(|sample| sample.audible.abs() > 0.01);
            }
            assert!(heard_monitor);
        }
    }

    #[test]
    fn remote_echo_still_cancels_while_live_monitor_is_excluded() {
        let source = std::array::from_fn(|index| (index as f32 * 0.1).sin() * 0.3);
        let echo = source.map(|sample| sample * 0.5);
        let mut resampler = PlaybackResampler::new(48_000).unwrap();
        let mut aec = finch_audio::EchoCanceller::new().unwrap();
        let mut final_energy = 0.0;
        for _ in 0..150 {
            let played = resampler.process(&source, &[0.05; FRAME_SIZE_48K]).unwrap();
            let reference = std::array::from_fn(|index| played[index].echo_reference);
            assert_eq!(reference, source, "remote/screen/test reference must remain available");
            aec.push_reference(&reference).unwrap();
            let mut captured = echo;
            aec.process_capture(&mut captured).unwrap();
            final_energy = captured.iter().map(|sample| sample * sample).sum::<f32>();
        }
        let original_energy = echo.iter().map(|sample| sample * sample).sum::<f32>();
        assert!(final_energy < original_energy * 0.1, "far-end echo cancellation must remain active");
    }

    #[test]
    fn playback_headroom_keeps_small_callbacks_low_and_large_requests_bounded() {
        assert_eq!(playback_headroom(960, 480), 2880);
        assert_eq!(playback_headroom(960, 4800), 5760);
        assert_eq!(playback_headroom(960, usize::MAX), RING_BUFFER_CAPACITY - 960);
    }

    #[test]
    fn playback_queue_supplies_large_device_callbacks_without_periodic_silence() {
        for rate in [8_000usize, 16_000, 44_100, 48_000, 96_000] {
            let chunk = rate / 50;
            for callback in [rate / 100, rate / 10] {
                let (mut producer, mut consumer) = HeapRb::<f32>::new(RING_BUFFER_CAPACITY).split();
                for _ in 0..5 {
                    while producer.occupied_len() < playback_headroom(chunk, callback)
                        && producer.vacant_len() >= chunk {
                        for _ in 0..chunk { producer.try_push(0.25).unwrap(); }
                    }
                    for _ in 0..callback {
                        assert_eq!(consumer.try_pop(), Some(0.25),
                            "{rate} Hz callback of {callback} frames must not insert silence");
                    }
                }
            }
        }
    }

    #[test]
    fn mic_test_records_then_plays_every_frame_once() {
        let mut test = MicTest::new(1);
        assert!(test.next_playback_frame().is_none(), "nothing plays while recording");
        for _ in 0..60 {
            test.record(&[0.1f32; FRAME_SIZE_48K], true, true); // extra frames are ignored
        }
        assert!(test.recording_done());
        assert!(!test.finished());
        let mut played = 0;
        while !test.finished() {
            assert!(test.next_playback_frame().is_some());
            played += 1;
        }
        assert_eq!(played, 50, "one second is fifty 20 ms frames");
        assert!(test.next_playback_frame().is_none());
    }

    #[test]
    fn mic_test_length_is_limited() {
        assert_eq!(MicTest::new(0).wanted_frames, 50);
        assert_eq!(MicTest::new(99).wanted_frames, 500);
    }

    #[test]
    fn mic_test_silences_rejected_frames_and_keeps_local_onset() {
        let mut test = MicTest::new(1);
        for _ in 0..4 { test.record(&[0.25; FRAME_SIZE_48K], false, true); }
        test.record(&[0.5; FRAME_SIZE_48K], true, true);
        test.record(&[0.75; FRAME_SIZE_48K], true, false);
        for _ in 6..50 { test.record(&[0.8; FRAME_SIZE_48K], false, true); }
        assert_eq!(test.next_playback_frame().unwrap()[0], 0.25);
        for _ in 1..4 { assert_eq!(test.next_playback_frame().unwrap()[0], 0.25); }
        assert_eq!(test.next_playback_frame().unwrap()[0], 0.5);
        assert_eq!(test.next_playback_frame().unwrap()[0], 0.0, "PTT-closed frame stays silent");
        assert_eq!(test.next_playback_frame().unwrap()[0], 0.0, "rejected frames stay silent");
    }

    #[test]
    fn microphone_test_suspends_monitor_without_changing_saved_choice() {
        let settings = crate::state::VoiceSettings::default();
        let mut requested = settings.snapshot();
        requested.monitor = true;
        settings.apply(&requested);
        assert!(effective_monitor(&settings, false));
        assert!(!effective_monitor(&settings, true));
        assert!(settings.snapshot().monitor);
        requested.monitor = false;
        settings.apply(&requested);
        assert!(!effective_monitor(&settings, true));
        assert!(!effective_monitor(&settings, false));
        requested.monitor = true;
        settings.apply(&requested);
        assert!(!effective_monitor(&settings, true));
        assert!(effective_monitor(&settings, false), "the user's latest choice takes effect when the test ends");
    }

    #[test]
    fn test_audio_engine_lifecycle() {
        let mut engine = AudioEngine::new();
        assert!(!engine.is_running());
        engine.stop();
        assert!(!engine.is_running());
    }

    #[test]
    fn test_ringbuf_transfer() {
        use ringbuf::traits::*;
        use ringbuf::HeapRb;

        let rb = HeapRb::<f32>::new(1024);
        let (mut prod, mut cons) = rb.split();

        assert_eq!(prod.vacant_len(), 1024);
        assert_eq!(cons.occupied_len(), 0);

        for i in 0..100 {
            assert!(prod.try_push(i as f32).is_ok());
        }

        assert_eq!(cons.occupied_len(), 100);

        for i in 0..100 {
            assert_eq!(cons.try_pop(), Some(i as f32));
        }

        assert_eq!(cons.occupied_len(), 0);
    }

    #[test]
    fn deafen_discards_queued_audio_and_feeds_only_silence_to_aec() {
        let (mut producer, mut playback) = HeapRb::<PlaybackSample>::new(1024).split();
        let (mut reference, mut received) = HeapRb::<f32>::new(1024).split();
        for _ in 0..960 { assert!(producer.try_push(playback_sample(0.3, 0.2)).is_ok()); }
        let mut data = [0.75; 32];
        render_deafened(&mut data, 2, &mut playback, &mut reference);
        assert!(data.iter().all(|sample| *sample == 0.0));
        assert_eq!(playback.occupied_len(), 0, "undeafen must not replay pre-deafen samples");
        assert_eq!(received.occupied_len(), 16);
        while let Some(sample) = received.try_pop() { assert_eq!(sample, 0.0); }
    }

    /// A briefly busy mixer (the network task is pushing a packet) must delay the frame,
    /// not replace it with silence (AUD-023).
    #[test]
    fn test_mixer_contention_does_not_insert_silence() {
        use finch_audio::OpusVoiceEncoder;
        use finch_protocol::media::MediaHeader;

        let mixer = std::sync::Arc::new(tokio::sync::Mutex::new(AudioMixer::new()));
        let now = Instant::now();
        let payload = OpusVoiceEncoder::new(64).unwrap().encode(&[0.3f32; FRAME_SIZE_48K]).unwrap();
        {
            let mut m = mixer.blocking_lock();
            for seq in 0..4u16 {
                let header = MediaHeader::new_audio(1, 7, seq, seq as u32 * 20, false, false);
                m.on_audio_packet(&header, payload.clone(), now);
            }
            let mut warmup = [0.0f32; FRAME_SIZE_48K];
            m.mix_frame(&mut warmup, now);
        }

        // Another thread holds the mixer for 30 ms, like the datagram task does.
        let holder = mixer.clone();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _guard = holder.blocking_lock();
            let _ = locked_tx.send(());
            std::thread::sleep(Duration::from_millis(30));
        });
        locked_rx.recv().unwrap();

        let mut frame = [0.0f32; FRAME_SIZE_48K];
        mix_next_frame(&mixer, &mut frame, now);
        handle.join().unwrap();
        assert!(frame.iter().any(|&s| s != 0.0), "a silent frame was inserted while the mixer was busy");
    }

    #[test]
    fn native_failures_stop_once_after_three_consecutive_frames() {
        let mut failures = ProcessingFailures::default();
        assert!(!failures.failed()); assert!(!failures.failed());
        failures.succeeded();
        assert!(!failures.failed()); assert!(!failures.failed());
        assert!(failures.failed());
        assert!(!failures.failed());
    }
    #[test]
    fn capture_source_uses_saved_mode_and_threshold_at_creation() {
        let settings = crate::state::VoiceSettings::default();
        let mut wanted = settings.snapshot();
        wanted.voice_enhancement = false;
        wanted.vad_threshold_db = -52.0;
        settings.apply(&wanted);
        let config = capture_config(7, &settings);
        assert!(!config.voice_enhancement);
        assert_eq!(config.vad_threshold_db, -52.0);
        assert_eq!(config.channel_short_id, 7);
    }
}
