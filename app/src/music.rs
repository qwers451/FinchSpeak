//! Synchronized local file playback. The server sends state, never decoded PCM.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use finch_audio::music::MusicDecoder;
use finch_client_core::api::ApiClient;
use finch_protocol::models::{MusicRoomState, MusicTrack, Playing};
use ringbuf::{HeapCons, HeapRb, traits::{Consumer, Producer, Split}};
use tokio::time::Instant;
use uuid::Uuid;

use crate::music_clock::{needs_resync, target_position};

const CACHE_LIMIT: u64 = 500 * 1024 * 1024;
const RING_FRAMES: usize = 48_000;
const CHUNK_FRAMES: usize = 960;
const MAX_RTT_MS: u64 = 10_000;

pub type OnError = Arc<dyn Fn(String) + Send + Sync>;
type Download = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
/// Writes the whole track to `dest`. Injected so the transport can be tested without a server.
type Fetch = Arc<dyn Fn(Uuid, PathBuf) -> Download + Send + Sync>;

#[derive(Clone)]
struct Backend {
    cache: PathBuf,
    fetch: Fetch,
    on_error: OnError,
}

#[derive(Clone)]
struct Target {
    playing: Playing,
    server_ms: u64,
    rtt_ms: u64,
    received: Instant,
}

impl Target {
    fn position(&self) -> u64 {
        let elapsed = self.received.elapsed().as_millis() as u64;
        let server_now = self.server_ms.saturating_add(self.rtt_ms / 2).saturating_add(elapsed);
        target_position(&self.playing, server_now).min(self.playing.entry.track.duration_ms)
    }
}

struct Stream {
    entry: Uuid,
    start_ms: u64,
    frames: Arc<AtomicU64>,
    ended: Arc<AtomicBool>,
}

impl Stream {
    /// Milliseconds of audio the mixer has actually played (48 frames per millisecond).
    fn position(&self) -> u64 {
        self.start_ms.saturating_add(self.frames.load(Ordering::Relaxed) / 48)
    }
}

#[derive(Default)]
struct Transport {
    room: Option<Uuid>,
    target: Option<Target>,
    stream: Option<Stream>,
    loading: bool,
}

struct Output {
    consumer: HeapCons<[f32; 2]>,
    frames: Arc<AtomicU64>,
}

/// Opens the decoder at `initial`, then re-reads the latest wanted position. A seek that arrived
/// while the file was opening reopens the decoder before any audio can reach the output.
fn prepare_decoder(path: &Path, initial: u64, latest: impl Fn() -> Result<u64, String>) -> Result<(MusicDecoder, u64), String> {
    let decoder = MusicDecoder::open(path, initial)?;
    let current = latest()?;
    if current == initial {
        return Ok((decoder, initial));
    }
    Ok((MusicDecoder::open(path, current)?, current))
}

pub struct MusicPlayer {
    transport: Mutex<Transport>,
    output: Mutex<Option<Output>>,
    generation: AtomicU64,
    paused: AtomicBool,
    volume: AtomicU32,
    job: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Default for MusicPlayer {
    fn default() -> Self {
        Self {
            transport: Mutex::new(Transport::default()),
            output: Mutex::new(None),
            generation: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(0.5f32.to_bits()),
            job: Mutex::new(None),
        }
    }
}

impl MusicPlayer {
    pub fn set_volume(&self, volume: f32) {
        self.volume.store(if volume.is_finite() { volume.clamp(0.0, 1.0).to_bits() } else { 0.5f32.to_bits() }, Ordering::Relaxed);
    }

    pub fn set_room(&self, room: Option<Uuid>) {
        let Ok(mut transport) = self.transport.lock() else { return };
        if transport.room == room {
            return;
        }
        self.cancel();
        *transport = Transport { room, ..Default::default() };
    }

    /// Keep membership for automatic reconnect, while retiring every old decoder and download.
    pub fn suspend(&self) {
        let Ok(mut transport) = self.transport.lock() else { return };
        self.cancel();
        transport.target = None;
        transport.stream = None;
        transport.loading = false;
    }

    /// Retires the current generation: decoder, download and drift monitor stop, queued audio is dropped.
    fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_output();
    }

    fn stop_output(&self) {
        if let Ok(mut output) = self.output.lock() {
            *output = None;
        }
        if let Ok(mut job) = self.job.lock() {
            if let Some(job) = job.take() {
                job.abort();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn receive(self: &Arc<Self>, channel: Uuid, state: MusicRoomState, server_ms: u64, rtt_ms: u64,
        api: ApiClient, cache: PathBuf, on_error: OnError) {
        let backend = Backend { cache, fetch: api_fetch(api), on_error };
        self.receive_with(channel, state, server_ms, rtt_ms, backend);
    }

    fn receive_with(self: &Arc<Self>, channel: Uuid, state: MusicRoomState, server_ms: u64, rtt_ms: u64, backend: Backend) {
        let restart = {
            let Ok(mut transport) = self.transport.lock() else { return };
            if transport.room != Some(channel) {
                return;
            }
            let Some(playing) = state.playing else {
                self.cancel();
                transport.target = None;
                transport.stream = None;
                transport.loading = false;
                return;
            };
            let target = Target { playing, server_ms, rtt_ms: rtt_ms.min(MAX_RTT_MS), received: Instant::now() };
            let same_entry = transport.target.as_ref().is_some_and(|old| old.playing.entry.entry_id == target.playing.entry.entry_id);
            self.paused.store(target.playing.paused, Ordering::SeqCst);
            // A download in progress already reads the newest target when it finishes.
            let behind = match &transport.stream {
                Some(stream) => needs_resync(stream.position(), target.position()),
                None => true,
            };
            let restart = !same_entry || (!transport.loading && behind);
            transport.target = Some(target);
            restart
        };
        if restart {
            self.launch(backend);
        }
    }

    fn launch(self: &Arc<Self>, backend: Backend) {
        let (generation, track) = {
            let Ok(mut transport) = self.transport.lock() else { return };
            let Some(target) = transport.target.as_ref() else { return };
            let track = target.playing.entry.track.clone();
            self.cancel();
            transport.stream = None;
            transport.loading = true;
            (self.generation.load(Ordering::SeqCst), track)
        };
        let player = self.clone();
        let handle = tokio::spawn(async move {
            if let Err(error) = run_track(player.clone(), generation, track, backend.clone()).await {
                player.fail(generation, error, &backend.on_error);
            }
        });
        if let Ok(mut job) = self.job.lock() {
            *job = Some(handle);
        }
    }

    /// Reports an error once and retires the generation, so no retry or drift check follows it.
    fn fail(&self, generation: u64, error: String, on_error: &OnError) {
        if self.generation.compare_exchange(generation, generation + 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        self.stop_output();
        if let Ok(mut transport) = self.transport.lock() {
            transport.loading = false;
            transport.stream = None;
        }
        on_error(error);
    }

    /// Entry and the position the room wants right now.
    fn current_position(&self) -> Result<(Uuid, u64), String> {
        let transport = self.transport.lock().map_err(|_| "Music player unavailable".to_string())?;
        let target = transport.target.as_ref().ok_or_else(|| "Music stopped".to_string())?;
        Ok((target.playing.entry.entry_id, target.position()))
    }

    fn start_decoder(self: &Arc<Self>, path: PathBuf, generation: u64, on_error: OnError) -> Result<(), String> {
        let (entry, start) = self.current_position()?;
        let player = self.clone();
        std::thread::Builder::new().name("finch-music-decoder".into()).spawn(move || {
            if let Err(error) = player.decode(&path, entry, start, generation) {
                player.fail(generation, error, &on_error);
            }
        }).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn decode(&self, path: &Path, entry: Uuid, start: u64, generation: u64) -> Result<(), String> {
        let (mut decoder, start) = prepare_decoder(path, start, || self.current_position().map(|(_, position)| position))?;
        let (mut producer, consumer) = HeapRb::<[f32; 2]>::new(RING_FRAMES).split();
        let frames = Arc::new(AtomicU64::new(0));
        let ended = Arc::new(AtomicBool::new(false));
        {
            let mut transport = self.transport.lock().map_err(|_| "Music player unavailable")?;
            if self.generation.load(Ordering::SeqCst) != generation {
                return Ok(());
            }
            let mut output = self.output.lock().map_err(|_| "Music output unavailable")?;
            *output = Some(Output { consumer, frames: frames.clone() });
            transport.stream = Some(Stream { entry, start_ms: start, frames, ended: ended.clone() });
            transport.loading = false;
        }
        let mut chunk = [[0.0; 2]; CHUNK_FRAMES];
        while self.generation.load(Ordering::SeqCst) == generation {
            let n = decoder.read_frames(&mut chunk)?;
            if n == 0 {
                ended.store(true, Ordering::SeqCst);
                break;
            }
            for frame in &chunk[..n] {
                while self.generation.load(Ordering::SeqCst) == generation {
                    if producer.try_push(*frame).is_ok() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        Ok(())
    }

    /// Called by the audio worker before output resampling/AEC reference construction.
    /// cpal callbacks never access this mutex or allocate memory.
    pub fn mix_frame(&self, left: &mut [f32], right: &mut [f32], deafened: bool) {
        if self.paused.load(Ordering::SeqCst) {
            return;
        }
        let gain = if deafened { 0.0 } else { f32::from_bits(self.volume.load(Ordering::Relaxed)) };
        let Ok(mut output) = self.output.lock() else { return };
        let Some(output) = output.as_mut() else { return };
        let mut played = 0;
        for (left, right) in left.iter_mut().zip(right) {
            if let Some(frame) = output.consumer.try_pop() {
                *left += frame[0] * gain;
                *right += frame[1] * gain;
                played += 1;
            }
        }
        output.frames.fetch_add(played, Ordering::Relaxed);
    }
}

/// Downloads, waits for the file, starts the decoder at the position current at that moment, and
/// keeps the position in step with the room until the generation changes.
async fn run_track(player: Arc<MusicPlayer>, generation: u64, track: MusicTrack, backend: Backend) -> Result<(), String> {
    let location = backend.cache.clone();
    let metadata = track.clone();
    let path = tokio::task::spawn_blocking(move || prepare_cache(&location, &metadata)).await.map_err(|e| e.to_string())??;
    let cached = tokio::fs::metadata(&path).await.is_ok_and(|meta| meta.len() == track.size_bytes);
    if !cached {
        let partial = path.with_extension(format!("{}.{}.part", track.format, Uuid::new_v4()));
        let downloaded = (backend.fetch)(track.id, partial.clone()).await;
        if player.generation.load(Ordering::SeqCst) != generation {
            let _ = tokio::fs::remove_file(&partial).await;
            return Ok(());
        }
        if let Err(error) = downloaded {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(error);
        }
        let size = tokio::fs::metadata(&partial).await.map_err(|e| e.to_string())?.len();
        if size != track.size_bytes {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err("Music download has the wrong size".into());
        }
        tokio::fs::rename(&partial, &path).await.map_err(|e| e.to_string())?;
    }
    if player.generation.load(Ordering::SeqCst) != generation {
        return Ok(());
    }
    player.start_decoder(path, generation, backend.on_error.clone())?;
    // State changes are sparse. Check drift locally so an output underrun or device reopen cannot
    // leave us permanently behind the other listeners.
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if player.generation.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        let drifted = player.transport.lock().ok().is_some_and(|transport| match (&transport.target, &transport.stream) {
            (Some(target), Some(stream)) if !target.playing.paused && !stream.ended.load(Ordering::SeqCst)
                && stream.entry == target.playing.entry.entry_id => needs_resync(stream.position(), target.position()),
            _ => false,
        });
        if drifted {
            player.launch(backend);
            return Ok(());
        }
    }
}

/// Downloads through the pinned REST client. A failure is reported through `music-error` by the caller;
/// the rest of the room is unaffected.
fn api_fetch(api: ApiClient) -> Fetch {
    Arc::new(move |track_id: Uuid, dest: PathBuf| -> Download {
        let api = api.clone();
        Box::pin(async move {
            api.download_music_file(track_id, &dest).await.map(|_| ()).map_err(|e| e.to_string())
        })
    })
}

fn prepare_cache(root: &Path, track: &MusicTrack) -> Result<PathBuf, String> {
    if !matches!(track.format.as_str(), "mp3" | "ogg" | "flac" | "wav") || track.size_bytes == 0 || track.size_bytes > CACHE_LIMIT {
        return Err("Music file is too large or has an unsupported format".into());
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let path = root.join(format!("{}.{}", track.id, track.format));
    let mut files = Vec::new();
    let mut total = 0u64;
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let file = entry.path();
        // Downloads in progress are never evicted; finished files are counted against the limit.
        if !meta.is_file() || file == path || file.extension().and_then(|ext| ext.to_str()) == Some("part") {
            continue;
        }
        total = total.saturating_add(meta.len());
        files.push((meta.modified().unwrap_or(UNIX_EPOCH), file, meta.len()));
    }
    files.sort_by_key(|file| file.0);
    for (_, old, size) in files {
        if total.saturating_add(track.size_bytes) <= CACHE_LIMIT {
            break;
        }
        // A file still open in a decoder cannot be removed on Windows; keep it and try the next one.
        if std::fs::remove_file(&old).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
        let _ = file.set_modified(SystemTime::now());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use finch_protocol::models::QueueEntry;

    const FIXTURE: &[u8] = include_bytes!("../../crates/finch-audio/tests/fixtures/music/test.wav");

    fn track(size_bytes: u64) -> MusicTrack {
        MusicTrack {
            id: Uuid::new_v4(),
            uploader_id: Uuid::new_v4(),
            title: "Track".into(),
            artist: None,
            duration_ms: 60_000,
            size_bytes,
            format: "wav".into(),
            temporary: false,
            created_at: 0,
        }
    }

    fn playing(track: MusicTrack, position_ms: u64, at_server_ms: u64, paused: bool) -> Playing {
        Playing {
            entry: QueueEntry { entry_id: Uuid::new_v4(), added_by: Uuid::new_v4(), track },
            position_ms,
            at_server_ms,
            paused,
        }
    }

    fn state(playing: Playing) -> MusicRoomState {
        MusicRoomState { queue: vec![], playing: Some(playing) }
    }

    fn cache_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("finch-music-{name}-{}", Uuid::new_v4()))
    }

    fn noop_error() -> OnError {
        Arc::new(|_| {})
    }

    fn backend(cache: PathBuf, fetch: Fetch, on_error: OnError) -> Backend {
        Backend { cache, fetch, on_error }
    }

    /// Writes `bytes` to the partial file after `delay`, the way a slow download would.
    fn slow_fetch(bytes: Vec<u8>, delay: Duration) -> Fetch {
        Arc::new(move |_: Uuid, dest: PathBuf| -> Download {
            let bytes = bytes.clone();
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                tokio::fs::write(dest, bytes).await.map_err(|e| e.to_string())
            })
        })
    }

    /// A download that never finishes, so the state machine can be inspected while loading.
    fn pending_fetch() -> Fetch {
        Arc::new(|_: Uuid, _: PathBuf| -> Download { Box::pin(std::future::pending()) })
    }

    /// Stereo 48 kHz 16-bit silence, long enough to seek past the two-second fixtures.
    fn silent_wav(seconds: u32) -> Vec<u8> {
        let data_len = seconds * 48_000 * 4;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&48_000u32.to_le_bytes());
        out.extend_from_slice(&192_000u32.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(44 + data_len as usize, 0);
        out
    }

    #[tokio::test]
    async fn music_loading_uses_the_latest_state_and_ignores_other_rooms() {
        let player = Arc::new(MusicPlayer::default());
        let room = Uuid::new_v4();
        player.set_room(Some(room));
        let api = ApiClient::new("https://localhost:8443").unwrap();
        let track = playing(track(4096), 1000, 10_000, false);
        let cache = cache_dir("latest");
        let error = noop_error();
        player.receive(Uuid::new_v4(), state(track.clone()), 10_000, 0, api.clone(), cache.clone(), error.clone());
        assert!(player.transport.lock().unwrap().target.is_none());
        player.receive(room, state(track.clone()), 10_000, 0, api.clone(), cache.clone(), error.clone());
        let generation = player.generation.load(Ordering::SeqCst);
        let mut updated = track;
        updated.position_ms = 7000;
        updated.paused = true;
        player.receive(room, state(updated), 10_000, 0, api, cache, error);
        assert_eq!(player.generation.load(Ordering::SeqCst), generation, "a download is shared while state gets newer");
        assert_eq!(player.transport.lock().unwrap().target.as_ref().unwrap().position(), 7000);
        player.set_room(None);
        assert!(player.transport.lock().unwrap().target.is_none());
    }

    #[test]
    fn music_startup_rechecks_seek_and_pause_before_installing_output() {
        let path = std::env::temp_dir().join(format!("finch-startup-{}.wav", Uuid::new_v4()));
        std::fs::write(&path, FIXTURE).unwrap();
        let (mut decoder, start) = prepare_decoder(&path, 0, || Ok(1000)).unwrap();
        assert_eq!(start, 1000, "a seek received during open changes the decoder before output becomes visible");
        let mut frames = [[0.0; 2]; 960];
        assert_eq!(decoder.read_frames(&mut frames).unwrap(), 960);
        let (_, start) = prepare_decoder(&path, 1000, || Ok(100)).unwrap();
        assert_eq!(start, 100, "a backwards seek reopens the decoder");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn music_decoder_error_stops_drift_retries_and_reports_once() {
        let player = Arc::new(MusicPlayer::default());
        player.set_room(Some(Uuid::new_v4()));
        let generation = player.generation.load(Ordering::SeqCst);
        let count = Arc::new(AtomicU64::new(0));
        let errors = count.clone();
        let error: OnError = Arc::new(move |_| {
            errors.fetch_add(1, Ordering::Relaxed);
        });
        let (mut producer, consumer) = HeapRb::new(4).split();
        producer.try_push([0.2, 0.2]).unwrap();
        *player.output.lock().unwrap() = Some(Output { consumer, frames: Arc::new(AtomicU64::new(0)) });
        player.fail(generation, "bad track".into(), &error);
        player.fail(generation, "late decoder error".into(), &error);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_ne!(player.generation.load(Ordering::SeqCst), generation, "the drift monitor must retire");
        assert!(player.output.lock().unwrap().is_none());
    }

    #[test]
    fn music_stereo_mix_respects_local_gain_pause_and_deafen() {
        let player = MusicPlayer::default();
        let (mut producer, consumer) = HeapRb::new(4).split();
        let frames = Arc::new(AtomicU64::new(0));
        *player.output.lock().unwrap() = Some(Output { consumer, frames: frames.clone() });
        producer.try_push([0.4, -0.2]).unwrap();
        player.paused.store(true, Ordering::SeqCst);
        let (mut left, mut right) = ([0.1], [0.2]);
        player.mix_frame(&mut left, &mut right, false);
        assert_eq!(frames.load(Ordering::Relaxed), 0);
        player.paused.store(false, Ordering::SeqCst);
        player.mix_frame(&mut left, &mut right, false);
        assert!((left[0] - 0.3).abs() < 0.001);
        assert!((right[0] - 0.1).abs() < 0.001);
        producer.try_push([0.4, -0.2]).unwrap();
        let (mut left, mut right) = ([0.0], [0.0]);
        player.mix_frame(&mut left, &mut right, true);
        assert_eq!((left, right), ([0.0], [0.0]));
        assert_eq!(frames.load(Ordering::Relaxed), 2, "deafen consumes the timeline without playing sound");
        player.mix_frame(&mut left, &mut right, false);
        assert_eq!((left, right), ([0.0], [0.0]), "underrun is silent");
    }

    #[test]
    fn music_leaving_room_retires_queued_audio_and_old_decoder_generation() {
        let player = MusicPlayer::default();
        player.set_room(Some(Uuid::new_v4()));
        let before = player.generation.load(Ordering::SeqCst);
        player.set_room(None);
        assert!(player.generation.load(Ordering::SeqCst) > before);
        assert!(player.transport.lock().unwrap().target.is_none());
        assert!(player.output.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn late_join_starts_from_the_position_when_the_file_is_ready() {
        let bytes = silent_wav(13);
        let player = Arc::new(MusicPlayer::default());
        let room = Uuid::new_v4();
        player.set_room(Some(room));
        let mut wanted = track(bytes.len() as u64);
        wanted.duration_ms = 13_000;
        let errors = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = errors.clone();
        let on_error: OnError = Arc::new(move |message| {
            if let Ok(mut list) = sink.lock() {
                list.push(message);
            }
        });
        let cache = cache_dir("late-join");
        let fetch = slow_fetch(bytes, Duration::from_secs(2));
        player.receive_with(room, state(playing(wanted, 10_000, 10_000, false)), 10_000, 0, backend(cache.clone(), fetch, on_error));
        let mut start = None;
        for _ in 0..400 {
            start = player.transport.lock().unwrap().stream.as_ref().map(|stream| stream.start_ms);
            if start.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        player.set_room(None);
        let _ = std::fs::remove_dir_all(&cache);
        assert!(errors.lock().unwrap().is_empty(), "the download should succeed");
        let start = start.expect("the decoder should start once the file is ready");
        assert!((11_900..=12_500).contains(&start), "started at {start} ms, expected about 12000 ms");
    }

    #[tokio::test]
    async fn a_failed_download_reports_once_and_leaves_the_room_intact() {
        let player = Arc::new(MusicPlayer::default());
        let room = Uuid::new_v4();
        player.set_room(Some(room));
        let count = Arc::new(AtomicU64::new(0));
        let errors = count.clone();
        let on_error: OnError = Arc::new(move |_| {
            errors.fetch_add(1, Ordering::Relaxed);
        });
        let failing: Fetch = Arc::new(|_: Uuid, _: PathBuf| -> Download { Box::pin(async { Err("network down".to_string()) }) });
        let cache = cache_dir("failed");
        let wanted = track(4096);
        player.receive_with(room, state(playing(wanted, 0, 0, false)), 0, 0, backend(cache.clone(), failing, on_error));
        for _ in 0..200 {
            if count.load(Ordering::Relaxed) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = std::fs::remove_dir_all(&cache);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        let transport = player.transport.lock().unwrap();
        assert_eq!(transport.room, Some(room), "a failed track keeps the room membership");
        assert!(!transport.loading);
    }

    #[tokio::test]
    async fn a_newer_state_while_loading_does_not_start_a_second_download() {
        let player = Arc::new(MusicPlayer::default());
        let room = Uuid::new_v4();
        player.set_room(Some(room));
        let first = playing(track(4096), 0, 0, false);
        let cache = cache_dir("shared");
        let fetch = pending_fetch();
        let error = noop_error();
        player.receive_with(room, state(first.clone()), 0, 0, backend(cache.clone(), fetch.clone(), error.clone()));
        let generation = player.generation.load(Ordering::SeqCst);
        // Same entry, newer position: the download in progress is reused.
        let mut newer = first;
        newer.position_ms = 5_000;
        player.receive_with(room, state(newer), 0, 0, backend(cache.clone(), fetch, error));
        assert_eq!(player.generation.load(Ordering::SeqCst), generation);
        player.set_room(None);
        let _ = std::fs::remove_dir_all(&cache);
    }
}
