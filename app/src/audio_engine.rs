//! Audio engine managing microphone capture and speaker playback via cpal.
//!
//! All `cpal::Stream` instances and DSP processing (rubato resampling, AEC,
//! noise suppression, VAD, and Opus codec) run in a dedicated background worker thread.
//! The audio hardware callbacks communicate with the worker thread strictly via lock-free
//! SPSC ring buffers (`ringbuf::HeapRb`) without mutex locks or memory allocations.

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

/// Capacity of lock-free ring buffers (16384 samples ~ 341 ms at 48 kHz).
const RING_BUFFER_CAPACITY: usize = 16384;

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

/// The meter is updated every second frame: 25 times a second.
const MIC_LEVEL_EVERY_FRAMES: u32 = 2;

/// "Test my microphone": a few seconds of processed microphone are recorded, then played back.
struct MicTest {
    wanted_frames: usize,
    frames: Vec<[f32; FRAME_SIZE_48K]>,
    played: usize,
}

impl MicTest {
    fn new(seconds: u32) -> Self {
        let wanted_frames = (seconds.clamp(1, 10) as usize) * 50;
        Self { wanted_frames, frames: Vec::with_capacity(wanted_frames), played: 0 }
    }

    fn record(&mut self, frame: &[f32; FRAME_SIZE_48K]) {
        if self.frames.len() < self.wanted_frames {
            self.frames.push(*frame);
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
                // Set from cpal's error callbacks when a stream dies (e.g. the headset is unplugged).
                let stream_failed = Arc::new(AtomicBool::new(false));

                // 1. Output (Playback) Ring Buffer & cpal Stream
                let playback_rb = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
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
                let mut output_resampler: Option<OutputResampler> = None;
                let mut output_sample_rate = 48000u32;

                let (output_device, output_choice) = open_output_device(selection.output.as_deref());
                report_fallback("output", &output_choice, &app_handle);
                if let Some(device) = output_device {
                    match device.default_output_config() {
                        Ok(config) => {
                            output_sample_rate = config.sample_rate().0;
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

                            output_resampler = match OutputResampler::new(output_sample_rate) {
                                Ok(resampler) => Some(resampler),
                                Err(e) => {
                                    error!("Failed to initialize OutputResampler: {}", e);
                                    Some(OutputResampler::Passthrough)
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
                                    if is_deafened_ref.load(Ordering::Relaxed) {
                                        data.fill(0.0);
                                        for _ in 0..data.len() / channels {
                                            let _ = reference_prod.try_push(0.0);
                                        }
                                        return;
                                    }
                                    for chunk in data.chunks_mut(channels) {
                                        let sample = playback_cons.try_pop().unwrap_or(0.0);
                                        let _ = reference_prod.try_push(sample);
                                        for out_ch in chunk.iter_mut() {
                                            *out_ch = sample;
                                        }
                                    }
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
                let capture_rb = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
                let (mut capture_prod, mut capture_cons) = capture_rb.split();

                let mut capture_stream: Option<Stream> = None;
                let mut input_resampler: Option<InputResampler> = None;

                let (input_device, input_choice) = open_input_device(selection.input.as_deref());
                report_fallback("input", &input_choice, &app_handle);
                if let Some(device) = input_device {
                    match device.default_input_config() {
                        Ok(config) => {
                            let sample_rate = config.sample_rate().0;
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
                            let stream_res = device.build_input_stream(
                                &config.into(),
                                move |data: &[f32], _| {
                                    if channels == 0 {
                                        return;
                                    }
                                    for frame in data.chunks(channels) {
                                        let mut sum = 0.0f32;
                                        for &s in frame {
                                            sum += s;
                                        }
                                        let mono = sum / (channels as f32);
                                        let _ = capture_prod.try_push(mono);
                                    }
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
                    error!("Both playback and capture audio streams failed to initialize");
                    is_running.store(false, Ordering::SeqCst);
                    continue;
                }

                // 3. DSP pipeline for worker thread processing
                let mut voice_capture = match VoiceCapture::new(VoiceCaptureConfig {
                    channel_short_id: channel_id,
                    ..VoiceCaptureConfig::default()
                }) {
                    Ok(capture) => Some(capture),
                    Err(e) => {
                        error!("Failed to create Opus voice encoder: {}", e);
                        None
                    }
                };

                let mut is_currently_speaking = false;
                // Meter and link quality for the UI, and the "test my microphone" loop.
                let mut level_frames: u32 = 0;
                let mut quality_tracker = QualityTracker::new();
                let mut last_quality_report = Instant::now();
                let mut mic_test: Option<MicTest> = None;
                // The processed microphone, played back to the user while "hear myself" is on.
                let mut monitor_queue: std::collections::VecDeque<[f32; FRAME_SIZE_48K]> = std::collections::VecDeque::new();
                let mut playback_frame_48k = [0.0f32; FRAME_SIZE_48K];
                let mut capture_frame_48k = [0.0f32; FRAME_SIZE_48K];
                let mut capture_raw_buf = Vec::with_capacity(4096);

                let target_output_chunk = ((output_sample_rate as usize * 20) / 1000).max(1);
                let target_playback_headroom = target_output_chunk * 3;

                // 4. Real-time active worker processing loop
                while is_running.load(Ordering::SeqCst) {
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
                        mic_test = Some(MicTest::new(requested));
                        let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "recording", "seconds": requested }));
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
                            while playback_prod.occupied_len() < target_playback_headroom
                                && playback_prod.vacant_len() >= target_output_chunk
                            {
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
                                    }
                                }
                                if let Some(own) = monitor_queue.pop_front() {
                                    for (out, mine) in playback_frame_48k.iter_mut().zip(own.iter()) {
                                        *out = (*out + *mine).clamp(-1.0, 1.0);
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

                                match resampler.process(&playback_frame_48k) {
                                    Ok(resampled) => {
                                        for &sample in &resampled {
                                            let _ = playback_prod.try_push(sample);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Output resampling error: {}", e);
                                        for _ in 0..target_output_chunk {
                                            let _ = playback_prod.try_push(0.0);
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
                            capture.set_stream_delay_ms(latency);
                            applied_latency_ms = latency;
                        }
                        if reference_cons.occupied_len() >= RING_BUFFER_CAPACITY - REFERENCE_OVERFLOW_MARGIN {
                            // The callback outran us and dropped samples: the stream is no longer
                            // contiguous, so forget the echo path instead of learning a wrong one.
                            warn!("Playout reference overflow; resetting echo canceller");
                            reference_cons.clear();
                            feeder.clear();
                            capture.reset_echo_canceller();
                        }
                        let mut played = [0.0f32; 1024];
                        loop {
                            let n = reference_cons.pop_slice(&mut played);
                            if n == 0 {
                                break;
                            }
                            feeder.push(&played[..n], |frame| capture.push_playback_reference(frame));
                        }
                    }

                    // B. Capture processing (capture_cons -> InputResampler -> AEC -> Noise -> VAD -> Opus)
                    if capture_stream.is_some() {
                        if let Some(resampler) = &mut input_resampler {
                            let needed = resampler.input_frames_needed();
                            if needed > 0 {
                                while capture_cons.occupied_len() >= needed {
                                    capture_raw_buf.clear();
                                    for _ in 0..needed {
                                        if let Some(s) = capture_cons.try_pop() {
                                            capture_raw_buf.push(s);
                                        }
                                    }

                                    if capture_raw_buf.len() != needed {
                                        break;
                                    }

                                    if state.is_muted.load(Ordering::Relaxed) {
                                        if is_currently_speaking {
                                            is_currently_speaking = false;
                                            let _ = app_handle.emit("local-speaking", false);
                                        }
                                        continue;
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
                                    capture.set_input_gain(state.voice_settings.input_gain());
                                    capture.set_vad_threshold_db(state.voice_settings.vad_threshold_db());
                                    // While testing the microphone nothing goes out to the others.
                                    capture.set_transmit_gate(state.voice_settings.transmit_open() && mic_test.is_none());
                                    let output =
                                        capture.process(&mut capture_frame_48k, sender_id, now_ms);
                                    level_frames += 1;
                                    if level_frames >= MIC_LEVEL_EVERY_FRAMES {
                                        level_frames = 0;
                                        let _ = app_handle.emit(
                                            "mic-level",
                                            serde_json::json!({ "db": output.level_db, "speaking": output.vad == VadDecision::Speaking }),
                                        );
                                    }
                                    if let Some(test) = &mut mic_test {
                                        test.record(&capture_frame_48k);
                                        if test.recording_done() {
                                            let _ = app_handle.emit("mic-test", serde_json::json!({ "phase": "playing" }));
                                        }
                                    }
                                    if state.voice_settings.monitor() {
                                        if monitor_queue.len() >= 5 {
                                            monitor_queue.pop_front();
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

                                    if !output.packets.is_empty() {
                                        if let Ok(conn_guard) = state.quic_connection.try_read() {
                                            if let Some(conn) = &*conn_guard {
                                                for packet in output.packets {
                                                    let _ = conn.send_datagram(packet);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    std::thread::sleep(Duration::from_millis(5));
                }

                if is_currently_speaking {
                    let _ = app_handle.emit("local-speaking", false);
                }

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
    fn mic_test_records_then_plays_every_frame_once() {
        let mut test = MicTest::new(1);
        assert!(test.next_playback_frame().is_none(), "nothing plays while recording");
        for _ in 0..60 {
            test.record(&[0.1f32; FRAME_SIZE_48K]); // extra frames are ignored
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
}
