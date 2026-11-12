//! Voice messages: a recording of the processed microphone, encoded to Ogg/Opus with a waveform.
//!
//! The audio worker copies each processed microphone frame into a lock-free ring buffer while a
//! recording is attached (see [`VoiceNoteTap`]); nothing is sent to a voice room. The recorder
//! thread drains that ring, keeps at most two minutes, and encodes the result when it stops.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use finch_audio::{OpusVoiceEncoder, FRAME_SIZE_48K, INTERNAL_SAMPLE_RATE};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::state::AppState;

/// Longest voice message; the recording stops by itself at this length.
pub const MAX_NOTE_MS: u64 = 120_000;
/// Values in the waveform sent with a voice message.
pub const WAVEFORM_BARS: usize = 64;
const OPUS_BITRATE_KBPS: u32 = 32;
/// Opus pre-skip of the encoder settings used here (the VoIP lookahead of libopus).
const PRE_SKIP: u16 = 312;
/// Ogg stream serial: any value is valid for a single logical stream.
const STREAM_SERIAL: u32 = 0x4649_4E43;
/// Frames per Ogg page: about one second of audio, so pages stay large and few.
const FRAMES_PER_PAGE: usize = 50;
/// Two seconds of samples between the audio worker and the recorder.
const TAP_CAPACITY: usize = 2 * INTERNAL_SAMPLE_RATE as usize;
/// How long the microphone may take to start delivering frames before the note fails.
const LIVE_TIMEOUT: Duration = Duration::from_secs(2);
const LEVEL_EVERY: Duration = Duration::from_millis(50);

pub const MICROPHONE_UNAVAILABLE: &str = "Микрофон недоступен";
const MICROPHONE_MUTED: &str = "Включите микрофон, чтобы записать голосовое";
const ALREADY_RECORDING: &str = "Запись уже идёт";
const NOT_RECORDING: &str = "Запись не идёт";
const EMPTY_RECORDING: &str = "Запись пустая";

/// A finished voice message, as the composer uploads it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VoiceNote {
    /// Ogg container with one Opus stream.
    pub bytes: Vec<u8>,
    pub duration_ms: u64,
    #[serde(serialize_with = "serialize_waveform")]
    pub waveform: [u8; WAVEFORM_BARS],
}

/// Serde implements arrays only up to 32 values, so the waveform goes out as a plain sequence.
fn serialize_waveform<S: serde::Serializer>(bars: &[u8; WAVEFORM_BARS], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(bars.iter())
}

/// Shared between the audio worker and the commands. The worker side never waits for anyone.
#[derive(Default)]
pub struct VoiceNoteTap {
    /// Id of the recording that gets frames; 0 when none does.
    session: AtomicU64,
    last_session: AtomicU64,
    /// True while the audio worker has a capture stream delivering frames.
    live: AtomicBool,
    pending: Mutex<Option<(u64, HeapProd<f32>)>>,
}

impl VoiceNoteTap {
    /// The recording whose frames the worker should take, 0 for none.
    pub fn session(&self) -> u64 {
        self.session.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        self.session() != 0
    }

    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }

    pub fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::SeqCst);
    }

    /// Recorder side: frames from now on are written to `producer`.
    fn attach(&self, producer: HeapProd<f32>) {
        let id = self.last_session.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut slot) = self.pending.lock() {
            *slot = Some((id, producer));
        }
        self.session.store(id, Ordering::SeqCst);
    }

    /// Recorder side: nothing is written any more.
    fn detach(&self) {
        self.session.store(0, Ordering::SeqCst);
        if let Ok(mut slot) = self.pending.lock() {
            *slot = None;
        }
    }

    /// Worker side: hands over the producer of `session` once. Skips the turn if the recorder
    /// holds the lock right now.
    fn take_pending(&self, session: u64) -> Option<HeapProd<f32>> {
        let mut slot = self.pending.try_lock().ok()?;
        match slot.take() {
            Some((id, producer)) if id == session => Some(producer),
            other => {
                *slot = other;
                None
            }
        }
    }
}

/// The audio worker's own handle on the recording it feeds. It lives on the worker thread, so it
/// survives the worker restarting its devices.
#[derive(Default)]
pub struct TapFeed {
    session: u64,
    producer: Option<HeapProd<f32>>,
}

impl TapFeed {
    /// Picks up a new recording, or drops the producer of one that has ended.
    pub fn refresh(&mut self, tap: &VoiceNoteTap) {
        let wanted = tap.session();
        if wanted == self.session && (wanted == 0 || self.producer.is_some()) {
            return;
        }
        self.session = wanted;
        self.producer = if wanted == 0 { None } else { tap.take_pending(wanted) };
    }

    /// Writes one processed frame. Samples that do not fit are dropped, never waited for.
    pub fn push(&mut self, frame: &[f32]) {
        if let Some(producer) = &mut self.producer {
            producer.push_slice(frame);
        }
    }
}

/// The recording in progress: its stop flag and the recorder thread that owns the result.
struct ActiveRecording {
    stop: std::sync::Arc<AtomicBool>,
    handle: JoinHandle<Result<VoiceNote, String>>,
}

/// Everything voice messages need from the app state.
#[derive(Default)]
pub struct VoiceNoteState {
    pub tap: VoiceNoteTap,
    recording: Mutex<Option<ActiveRecording>>,
}

/// Samples of a recording, at most [`MAX_NOTE_MS`] long.
#[derive(Default)]
pub struct NoteBuffer {
    samples: Vec<f32>,
}

impl NoteBuffer {
    fn max_samples() -> usize {
        (INTERNAL_SAMPLE_RATE as u64 * MAX_NOTE_MS / 1000) as usize
    }

    /// Appends samples; returns true once the limit is reached. Anything past it is dropped.
    pub fn push(&mut self, chunk: &[f32]) -> bool {
        let room = Self::max_samples().saturating_sub(self.samples.len());
        self.samples.extend_from_slice(&chunk[..chunk.len().min(room)]);
        self.is_full()
    }

    pub fn is_full(&self) -> bool {
        self.samples.len() >= Self::max_samples()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Encodes what was recorded.
    pub fn finish(&self) -> Result<VoiceNote, String> {
        if self.samples.is_empty() {
            return Err(EMPTY_RECORDING.to_string());
        }
        Ok(VoiceNote {
            bytes: encode_ogg_opus(&self.samples)?,
            duration_ms: duration_ms(self.samples.len()),
            waveform: waveform(&self.samples),
        })
    }
}

/// Length of `samples` at the internal rate, in whole milliseconds.
pub fn duration_ms(samples: usize) -> u64 {
    samples as u64 * 1000 / INTERNAL_SAMPLE_RATE as u64
}

/// Peak level per bucket, 64 buckets across the recording, 0 to 255.
pub fn waveform(samples: &[f32]) -> [u8; WAVEFORM_BARS] {
    let mut bars = [0u8; WAVEFORM_BARS];
    if samples.is_empty() {
        return bars;
    }
    for (index, bar) in bars.iter_mut().enumerate() {
        let start = index * samples.len() / WAVEFORM_BARS;
        let end = ((index + 1) * samples.len() / WAVEFORM_BARS).max(start + 1).min(samples.len());
        let mean_square = samples[start..end].iter().map(|s| s * s).sum::<f32>() / (end - start) as f32;
        // Speech is quiet next to full scale: four times the RMS fills the bar.
        let level = (mean_square.sqrt() * 4.0).clamp(0.0, 1.0);
        *bar = (level * 255.0).round() as u8;
    }
    bars
}

/// The OpusHead packet (RFC 7845, section 5.1) for mono 48 kHz input.
fn opus_head() -> Vec<u8> {
    let mut head = b"OpusHead".to_vec();
    head.push(1);
    head.push(1);
    head.extend_from_slice(&PRE_SKIP.to_le_bytes());
    head.extend_from_slice(&INTERNAL_SAMPLE_RATE.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes());
    head.push(0);
    head
}

/// The OpusTags packet: a vendor string and no user comments.
fn opus_tags() -> Vec<u8> {
    let vendor = b"FinchSpeak";
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    tags
}

fn container_error(error: impl std::fmt::Display) -> String {
    format!("Could not write the voice message: {error}")
}

/// Encodes mono 48 kHz samples as Ogg/Opus. The last page's granule position marks where the
/// audio ends, so the decoder drops the padding of the final frame.
pub fn encode_ogg_opus(samples: &[f32]) -> Result<Vec<u8>, String> {
    use ogg::writing::{PacketWriteEndInfo, PacketWriter};

    let mut encoder = OpusVoiceEncoder::new(OPUS_BITRATE_KBPS).map_err(|e| e.to_string())?;
    let mut writer = PacketWriter::new(Vec::new());
    writer.write_packet(opus_head(), STREAM_SERIAL, PacketWriteEndInfo::EndPage, 0).map_err(container_error)?;
    writer.write_packet(opus_tags(), STREAM_SERIAL, PacketWriteEndInfo::EndPage, 0).map_err(container_error)?;

    let frames = samples.len().div_ceil(FRAME_SIZE_48K);
    let mut frame = [0.0f32; FRAME_SIZE_48K];
    for index in 0..frames {
        let start = index * FRAME_SIZE_48K;
        let end = (start + FRAME_SIZE_48K).min(samples.len());
        frame.fill(0.0);
        frame[..end - start].copy_from_slice(&samples[start..end]);
        let packet = encoder.encode(&frame).map_err(|e| e.to_string())?;
        let granule = PRE_SKIP as u64 + end as u64;
        let end_info = if index + 1 == frames {
            PacketWriteEndInfo::EndStream
        } else if (index + 1) % FRAMES_PER_PAGE == 0 {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        writer.write_packet(packet.to_vec(), STREAM_SERIAL, end_info, granule).map_err(container_error)?;
    }
    Ok(writer.into_inner())
}

/// Waits until the audio worker delivers microphone frames, or the time runs out.
fn wait_until_live(tap: &VoiceNoteTap, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if tap.is_live() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Normalised loudness of the last chunks, for the meter while recording (0 to 1).
fn level_of(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return 0.0;
    }
    let mean_square = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
    (mean_square.sqrt() * 4.0).clamp(0.0, 1.0)
}

/// The recorder thread: drains the tap until stopped or at the limit, then encodes.
fn run_recorder(
    mut consumer: HeapCons<f32>,
    stop: std::sync::Arc<AtomicBool>,
    shared: std::sync::Arc<VoiceNoteState>,
    app: AppHandle,
) -> Result<VoiceNote, String> {
    let mut buffer = NoteBuffer::default();
    let mut chunk = vec![0.0f32; TAP_CAPACITY];
    let mut level_peak = 0.0f32;
    let mut level_due = Instant::now() + LEVEL_EVERY;
    let mut limit_reached = false;
    loop {
        let read = consumer.pop_slice(&mut chunk);
        if read > 0 {
            level_peak = level_peak.max(level_of(&chunk[..read]));
            if buffer.push(&chunk[..read]) {
                limit_reached = true;
            }
        }
        if Instant::now() >= level_due {
            level_due = Instant::now() + LEVEL_EVERY;
            let _ = app.emit("voice-note-level", serde_json::json!({ "level": level_peak }));
            level_peak = 0.0;
        }
        if limit_reached {
            let _ = app.emit("voice-note-limit", serde_json::json!({ "duration_ms": MAX_NOTE_MS }));
            break;
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if read == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    shared.tap.detach();
    // Frames the worker had already taken may still arrive for a moment.
    std::thread::sleep(Duration::from_millis(30));
    while !buffer.is_full() {
        let read = consumer.pop_slice(&mut chunk);
        if read == 0 {
            break;
        }
        buffer.push(&chunk[..read]);
    }
    buffer.finish()
}

/// Starts recording a voice message from the microphone. Opens the capture side the way the
/// microphone test does when no call is running.
pub async fn start(state: &AppState, app: AppHandle) -> Result<(), String> {
    if state.is_muted.load(Ordering::SeqCst) {
        return Err(MICROPHONE_MUTED.to_string());
    }
    {
        let mut slot = state.voice_note.recording.lock().map_err(|_| "voice note state poisoned".to_string())?;
        if slot.as_ref().is_some_and(|active| !active.handle.is_finished()) {
            return Err(ALREADY_RECORDING.to_string());
        }
        // A finished leftover (it reached the limit, and its stop was never called) is dropped.
        *slot = None;
    }

    let (producer, consumer) = HeapRb::<f32>::new(TAP_CAPACITY).split();
    state.voice_note.tap.attach(producer);
    {
        let mut engine = state.audio_engine.lock().await;
        if !engine.is_running() {
            if let Err(error) = engine.start_local_mic_test(state.clone(), app.clone()) {
                state.voice_note.tap.detach();
                return Err(format!("{MICROPHONE_UNAVAILABLE}: {error}"));
            }
        }
    }
    let live_shared = state.voice_note.clone();
    let live = tokio::task::spawn_blocking(move || wait_until_live(&live_shared.tap, LIVE_TIMEOUT))
        .await
        .map_err(|e| e.to_string())?;
    if !live {
        state.voice_note.tap.detach();
        return Err(MICROPHONE_UNAVAILABLE.to_string());
    }

    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let shared = state.voice_note.clone();
    let handle = std::thread::Builder::new()
        .name("finch-voice-note".into())
        .spawn(move || run_recorder(consumer, thread_stop, shared, app))
        .map_err(|e| {
            state.voice_note.tap.detach();
            format!("Could not start the recorder: {e}")
        })?;
    if let Ok(mut slot) = state.voice_note.recording.lock() {
        *slot = Some(ActiveRecording { stop, handle });
    }
    Ok(())
}

fn take_recording(state: &AppState) -> Result<Option<ActiveRecording>, String> {
    let mut slot = state.voice_note.recording.lock().map_err(|_| "voice note state poisoned".to_string())?;
    Ok(slot.take())
}

/// Ends the recording and returns the encoded message (the recording may already have stopped at
/// the limit).
pub async fn stop(state: &AppState) -> Result<VoiceNote, String> {
    let active = take_recording(state)?.ok_or_else(|| NOT_RECORDING.to_string())?;
    active.stop.store(true, Ordering::SeqCst);
    let joined = tokio::task::spawn_blocking(move || active.handle.join())
        .await
        .map_err(|e| e.to_string())?;
    match joined {
        Ok(result) => result,
        Err(_) => Err("The recording stopped unexpectedly".to_string()),
    }
}

/// Throws the recording away.
pub async fn cancel(state: &AppState) -> Result<(), String> {
    let Some(active) = take_recording(state)? else {
        return Ok(());
    };
    active.stop.store(true, Ordering::SeqCst);
    let _ = tokio::task::spawn_blocking(move || active.handle.join()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use finch_audio::OpusVoiceDecoder;
    use ogg::reading::PacketReader;

    const RATE: f32 = INTERNAL_SAMPLE_RATE as f32;

    fn sine(seconds: f32, amplitude: f32) -> Vec<f32> {
        (0..(seconds * RATE) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE).sin() * amplitude)
            .collect()
    }

    /// Reads the Ogg pages back: header packets, the decoded PCM and the last granule position.
    fn decode_back(bytes: &[u8]) -> (Vec<u8>, Vec<f32>, u64) {
        assert_eq!(&bytes[..4], b"OggS", "an Ogg file starts with a capture pattern");
        let mut reader = PacketReader::new(std::io::Cursor::new(bytes));
        let mut decoder = OpusVoiceDecoder::new().unwrap();
        let mut head = Vec::new();
        let mut pcm = Vec::new();
        let mut last_granule = 0;
        let mut index = 0;
        while let Some(packet) = reader.read_packet().unwrap() {
            last_granule = packet.absgp_page();
            if index == 0 {
                head = packet.data.clone();
            } else if index >= 2 {
                let mut out = [0.0f32; FRAME_SIZE_48K];
                let decoded = decoder.decode(&packet.data, &mut out).unwrap();
                pcm.extend_from_slice(&out[..decoded]);
            }
            index += 1;
        }
        (head, pcm, last_granule)
    }

    #[test]
    fn voice_note_three_seconds_encode_to_ogg_opus_with_matching_duration() {
        let samples = sine(3.0, 0.4);
        let bytes = encode_ogg_opus(&samples).unwrap();
        let (head, pcm, last_granule) = decode_back(&bytes);
        assert_eq!(&head[..8], b"OpusHead", "the first packet is OpusHead");
        assert_eq!(&head[12..16], &INTERNAL_SAMPLE_RATE.to_le_bytes(), "input rate is 48 kHz");
        let duration = (last_granule - PRE_SKIP as u64) * 1000 / INTERNAL_SAMPLE_RATE as u64;
        assert!((duration as i64 - 3000).abs() <= 40, "duration {duration} ms is not 3000 ms");
        assert!(pcm.len() >= samples.len(), "all audio is decoded");
        let mean_square: f32 = pcm[2000..].iter().map(|s| s * s).sum::<f32>() / (pcm.len() - 2000) as f32;
        assert!(mean_square.sqrt() > 0.1, "decoded audio is not silent: rms {}", mean_square.sqrt());
    }

    #[test]
    fn voice_note_waveform_has_64_values_and_loud_parts_are_higher() {
        let mut samples = vec![0.0f32; 48_000];
        samples.extend(sine(1.0, 0.8));
        samples.extend(vec![0.0f32; 48_000]);
        let bars = waveform(&samples);
        assert_eq!(bars.len(), WAVEFORM_BARS);
        assert_eq!(bars[0], 0, "a silent start gives a zero bar");
        assert_eq!(bars[WAVEFORM_BARS / 2], 255, "a loud middle gives a full bar");
        assert!(bars[WAVEFORM_BARS - 1] < bars[WAVEFORM_BARS / 2]);
    }

    #[test]
    fn voice_note_limit_is_exactly_two_minutes_of_input() {
        let mut buffer = NoteBuffer::default();
        let mut full_at = None;
        for second in 0..130 {
            let chunk = vec![0.1f32; INTERNAL_SAMPLE_RATE as usize];
            if buffer.push(&chunk) && full_at.is_none() {
                full_at = Some(second + 1);
            }
        }
        assert_eq!(full_at, Some(120), "the limit is reached at 120 s");
        assert_eq!(buffer.len(), NoteBuffer::max_samples());
        let note = buffer.finish().unwrap();
        assert_eq!(note.duration_ms, 120_000);
    }

    #[test]
    fn voice_note_empty_recording_is_an_error_not_an_empty_file() {
        assert_eq!(NoteBuffer::default().finish().unwrap_err(), EMPTY_RECORDING);
    }

    #[test]
    fn voice_note_without_microphone_is_reported_after_the_wait() {
        let tap = VoiceNoteTap::default();
        let started = Instant::now();
        assert!(!wait_until_live(&tap, Duration::from_millis(60)), "a missing microphone never becomes live");
        assert!(started.elapsed() >= Duration::from_millis(60));
        tap.set_live(true);
        assert!(wait_until_live(&tap, Duration::from_millis(60)));
    }

    #[test]
    fn voice_note_tap_hands_over_each_recording_once() {
        let tap = VoiceNoteTap::default();
        let mut feed = TapFeed::default();
        assert!(!tap.is_active());

        let (producer, mut consumer) = HeapRb::<f32>::new(64).split();
        tap.attach(producer);
        feed.refresh(&tap);
        feed.push(&[0.5; 8]);
        let mut out = [0.0f32; 16];
        assert_eq!(consumer.pop_slice(&mut out), 8, "the worker's frames reach the recorder");

        tap.detach();
        feed.refresh(&tap);
        feed.push(&[0.9; 8]);
        assert_eq!(consumer.pop_slice(&mut out), 0, "nothing is written after the recording ended");
    }

    #[test]
    fn voice_note_frames_are_dropped_when_the_ring_is_full() {
        let tap = VoiceNoteTap::default();
        let mut feed = TapFeed::default();
        let (producer, mut consumer) = HeapRb::<f32>::new(4).split();
        tap.attach(producer);
        feed.refresh(&tap);
        feed.push(&[0.1; 10]);
        let mut out = [0.0f32; 16];
        assert_eq!(consumer.pop_slice(&mut out), 4, "the worker never waits for a slow recorder");
    }
}
