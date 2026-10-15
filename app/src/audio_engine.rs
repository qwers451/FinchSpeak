//! Audio engine managing microphone capture and speaker playback via cpal.
//!
//! All `cpal::Stream` instances are owned by a dedicated background OS thread,
//! ensuring real-time priority and full `Send + Sync` safety across Tauri state.

use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use finch_audio::{
    InputResampler, OpusVoiceEncoder, OutputResampler, VadDecision, VoiceActivityDetector,
    FRAME_SIZE_48K,
};
use finch_protocol::media::{MediaHeader, MEDIA_HEADER_SIZE};
use tauri::Emitter;
use tracing::{error, info};

use crate::state::AppState;

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

// Explicitly safe because cmd_tx is Sender and is_running is Arc<AtomicBool>
unsafe impl Send for AudioEngine {}
unsafe impl Sync for AudioEngine {}

impl AudioEngine {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = channel::<AudioCommand>();
        let is_running = Arc::new(AtomicBool::new(false));
        let is_running_thread = is_running.clone();

        std::thread::Builder::new()
            .name("finch-audio-worker".into())
            .spawn(move || {
                let mut _capture_stream: Option<Stream> = None;
                let mut _playback_stream: Option<Stream> = None;

                while let Ok(cmd) = cmd_rx.recv() {
                    match cmd {
                        AudioCommand::Start {
                            channel_id,
                            state,
                            app_handle,
                        } => {
                            // Stop existing streams if any
                            _capture_stream = None;
                            _playback_stream = None;

                            let host = cpal::default_host();

                            // 1. Output (Playback) Stream
                            if let Some(device) = host.default_output_device() {
                                if let Ok(config) = device.default_output_config() {
                                    let sample_rate = config.sample_rate().0;
                                    let channels = config.channels() as usize;

                                    let mixer_ref = state.mixer.clone();
                                    let is_deafened_ref = state.is_deafened.clone();
                                    let app_handle_out = app_handle.clone();

                                    let mut resampler = OutputResampler::new(sample_rate)
                                        .unwrap_or(OutputResampler::Passthrough);
                                    let mut frame_48k = [0.0f32; FRAME_SIZE_48K];

                                    let err_fn = |err| error!("Audio output error: {}", err);
                                    if let Ok(stream) = device.build_output_stream(
                                        &config.into(),
                                        move |data: &mut [f32], _| {
                                            if is_deafened_ref.load(Ordering::Relaxed) {
                                                data.fill(0.0);
                                                return;
                                            }

                                            let speech_events = {
                                                if let Ok(mut mixer) = mixer_ref.try_lock() {
                                                    mixer.mix_frame(&mut frame_48k, Instant::now())
                                                } else {
                                                    frame_48k.fill(0.0);
                                                    Vec::new()
                                                }
                                            };

                                            for (peer_id, speaking) in speech_events {
                                                let _ = app_handle_out.emit(
                                                    "user-speaking",
                                                    serde_json::json!({
                                                        "peer_id": peer_id,
                                                        "is_speaking": speaking
                                                    }),
                                                );
                                            }

                                            if let Ok(resampled) = resampler.process(&frame_48k) {
                                                for (i, chunk) in data.chunks_mut(channels).enumerate() {
                                                    let sample = resampled.get(i).copied().unwrap_or(0.0);
                                                    for out_ch in chunk.iter_mut() {
                                                        *out_ch = sample;
                                                    }
                                                }
                                            } else {
                                                data.fill(0.0);
                                            }
                                        },
                                        err_fn,
                                        None,
                                    ) {
                                        if stream.play().is_ok() {
                                            _playback_stream = Some(stream);
                                        }
                                    }
                                }
                            }

                            // 2. Input (Capture) Stream
                            if let Some(device) = host.default_input_device() {
                                if let Ok(config) = device.default_input_config() {
                                    let sample_rate = config.sample_rate().0;
                                    let channels = config.channels() as usize;

                                    let mut resampler = InputResampler::new(sample_rate)
                                        .unwrap_or(InputResampler::Passthrough);
                                    let mut vad = VoiceActivityDetector::new(-45.0);
                                    let mut encoder = match OpusVoiceEncoder::new(64) {
                                        Ok(enc) => Some(enc),
                                        Err(e) => {
                                            error!("Failed to create Opus encoder: {}", e);
                                            None
                                        }
                                    };

                                    let is_muted_ref = state.is_muted.clone();
                                    let quic_conn_ref = state.quic_connection.clone();
                                    let app_handle_in = app_handle.clone();
                                    let seq_counter = Arc::new(AtomicU16::new(0));

                                    let mut accumulated_samples: Vec<f32> = Vec::new();
                                    let mut frame_48k = [0.0f32; FRAME_SIZE_48K];
                                    let mut is_currently_speaking = false;

                                    let err_fn = |err| error!("Audio input error: {}", err);
                                    if let Ok(stream) = device.build_input_stream(
                                        &config.into(),
                                        move |data: &[f32], _| {
                                            if is_muted_ref.load(Ordering::Relaxed) {
                                                if is_currently_speaking {
                                                    is_currently_speaking = false;
                                                    let _ = app_handle_in.emit("local-speaking", false);
                                                }
                                                return;
                                            }

                                            for frame in data.chunks(channels) {
                                                let mono_sample = frame.iter().sum::<f32>() / channels as f32;
                                                accumulated_samples.push(mono_sample);
                                            }

                                            let needed = resampler.input_frames_needed();
                                            while accumulated_samples.len() >= needed {
                                                let chunk: Vec<f32> = accumulated_samples.drain(..needed).collect();
                                                if resampler.process(&chunk, &mut frame_48k).is_ok() {
                                                    let vad_decision = vad.process(&frame_48k);
                                                    let should_send = vad_decision.should_transmit();

                                                    if vad_decision == VadDecision::Speaking {
                                                        if !is_currently_speaking {
                                                            is_currently_speaking = true;
                                                            let _ = app_handle_in.emit("local-speaking", true);
                                                        }
                                                    } else if vad_decision == VadDecision::Silence {
                                                        if is_currently_speaking {
                                                            is_currently_speaking = false;
                                                            let _ = app_handle_in.emit("local-speaking", false);
                                                        }
                                                    }

                                                    if should_send {
                                                        if let Some(enc) = &mut encoder {
                                                            if let Ok(opus_bytes) = enc.encode(&frame_48k) {
                                                                let seq = seq_counter.fetch_add(1, Ordering::Relaxed);
                                                                let now_ms = SystemTime::now()
                                                                    .duration_since(UNIX_EPOCH)
                                                                    .unwrap_or_default()
                                                                    .as_millis() as u32;

                                                                let is_silence = vad_decision.is_silence_packet();
                                                                let header = MediaHeader::new_audio(
                                                                    channel_id,
                                                                    0,
                                                                    seq,
                                                                    now_ms,
                                                                    is_silence,
                                                                    false,
                                                                );

                                                                let mut packet = vec![0u8; MEDIA_HEADER_SIZE + opus_bytes.len()];
                                                                let mut slice = &mut packet[..MEDIA_HEADER_SIZE];
                                                                header.write_to(&mut slice);
                                                                packet[MEDIA_HEADER_SIZE..].copy_from_slice(&opus_bytes);

                                                                if let Ok(conn_guard) = quic_conn_ref.try_read() {
                                                                    if let Some(conn) = &*conn_guard {
                                                                        let _ = conn.send_datagram(Bytes::from(packet));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        err_fn,
                                        None,
                                    ) {
                                        if stream.play().is_ok() {
                                            _capture_stream = Some(stream);
                                        }
                                    }
                                }
                            }

                            is_running_thread.store(true, Ordering::SeqCst);
                            info!("Audio worker started streams for channel {}", channel_id);
                        }
                        AudioCommand::Stop => {
                            _capture_stream = None;
                            _playback_stream = None;
                            is_running_thread.store(false, Ordering::SeqCst);
                            info!("Audio worker stopped all streams");
                        }
                    }
                }
            })
            .expect("Failed to spawn audio worker thread");

        Self { cmd_tx, is_running }
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub fn start(
        &mut self,
        channel_id: u32,
        state: AppState,
        app_handle: tauri::AppHandle,
    ) -> anyhow::Result<()> {
        self.cmd_tx
            .send(AudioCommand::Start {
                channel_id,
                state,
                app_handle,
            })
            .map_err(|e| anyhow::anyhow!("Failed to send start audio command: {}", e))?;
        Ok(())
    }

    pub fn stop(&mut self) {
        let _ = self.cmd_tx.send(AudioCommand::Stop);
    }
}
