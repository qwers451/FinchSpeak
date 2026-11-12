//! Screen sharing control and WebCodecs channel registration.

use std::sync::atomic::Ordering;

use finch_client_core::screen::{share_size, AudioPacketSource, ScreenAudioForwarder, FirstFrame, FrameSource, MonitorSource, ScreenSharer, ScreenViewer, ShareConfig, ShareHooks, ShareStats, WindowSource};
#[cfg(windows)]
use finch_client_core::screen::WgcSource;
use finch_protocol::messages::ClientMessage;
use finch_protocol::models::VideoCodec;
use finch_screen::{capture_monitor, capture_window, list_monitors, list_windows, thumbnail_jpeg, window_process_id, AudioSourceKind, ScreenAudioCapturer, ScreenAudioConfig, ThumbnailCache, WindowInfo};
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tracing::info;
use uuid::Uuid;

use crate::state::{AppState, ScreenShareReservation};

/// 60 FPS only when asked for; every other request (including none) shares at 30.
fn share_fps(requested: Option<u8>) -> u8 {
    if requested == Some(60) { 60 } else { 30 }
}

/// How long a capture session may take to show its first picture before it counts as having none yet.
const FIRST_PICTURE_WAIT: Duration = Duration::from_secs(2);

/// A capture session that can be asked for its first picture.
#[cfg(any(windows, test))]
trait PictureSession: FrameSource {
    fn first_frame(&mut self, wait: Duration) -> FirstFrame;
}

#[cfg(windows)]
impl PictureSession for WgcSource {
    fn first_frame(&mut self, wait: Duration) -> FirstFrame {
        WgcSource::first_frame(self, wait)
    }
}

/// Starts a share from a capture session. A picture gives the size of the picture. A window with no
/// picture yet (minimized) starts anyway, at the size it restores to, or the primary screen's size.
/// `None` means the session cannot be used and the snapshot capture should take over.
#[cfg(any(windows, test))]
fn start_picture_session<S: PictureSession>(
    mut session: S,
    is_window: bool,
    id: u32,
    restore_size: impl Fn(u32) -> Option<(u32, u32)>,
    primary_size: impl FnOnce() -> Option<(u32, u32)>,
) -> Option<(Box<dyn FrameSource>, u32, u32)> {
    match session.first_frame(FIRST_PICTURE_WAIT) {
        FirstFrame::Frame(frame) => Some((Box::new(session), frame.width, frame.height)),
        FirstFrame::NotYet if is_window => {
            let (width, height) = restore_size(id).or_else(primary_size)?;
            Some((Box::new(session), width, height))
        }
        // A screen with no picture, or a session that closed: the snapshot capture decides.
        FirstFrame::NotYet | FirstFrame::Closed => None,
    }
}

/// The size of the primary screen, for a window that has never shown a picture.
#[cfg(windows)]
fn primary_monitor_size() -> Option<(u32, u32)> {
    list_monitors().ok()?.into_iter().find(|m| m.is_primary).map(|m| (m.width, m.height))
}

/// The size a minimized window returns to (its normal placement), from the window handle.
#[cfg(windows)]
fn window_restore_size(id: u32) -> Option<(u32, u32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
    // SAFETY: WINDOWPLACEMENT is plain data, and GetWindowPlacement writes it before it is read.
    let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    // SAFETY: the handle is only queried; a stale handle makes the call fail, which is handled.
    if unsafe { GetWindowPlacement(id as usize as _, &mut placement) } == 0 {
        return None;
    }
    let rect = placement.rcNormalPosition;
    let width = u32::try_from(rect.right.saturating_sub(rect.left)).ok().filter(|w| *w > 0)?;
    let height = u32::try_from(rect.bottom.saturating_sub(rect.top)).ok().filter(|h| *h > 0)?;
    Some((width, height))
}

/// Without Windows there is no minimized window to restore, so the primary size is used.
#[cfg(not(windows))]
fn window_restore_size(_id: u32) -> Option<(u32, u32)> {
    None
}

/// The frame source for a screen or window and the size of its first picture. On Windows a capture
/// session is tried first (frames arrive as fast as the screen changes); if it cannot start, or a
/// screen shows no picture, the older snapshot capture takes over.
fn open_source(kind: &str, id: u32, fps: u8) -> Result<(Box<dyn FrameSource>, u32, u32), String> {
    open_source_with(kind, id, fps, window_restore_size)
}

/// [`open_source`] with the restore size of a window supplied by the caller.
fn open_source_with(kind: &str, id: u32, fps: u8, restore_size: impl Fn(u32) -> Option<(u32, u32)>) -> Result<(Box<dyn FrameSource>, u32, u32), String> {
    if kind != "monitor" && kind != "window" {
        return Err(format!("Unknown screen source kind: {kind}"));
    }
    #[cfg(windows)]
    {
        let session = if kind == "monitor" { WgcSource::monitor(id, fps) } else { WgcSource::window(id, fps) };
        match session {
            Ok(session) => {
                if let Some(started) = start_picture_session(session, kind == "window", id, restore_size, primary_monitor_size) {
                    return Ok(started);
                }
            }
            Err(e) => tracing::warn!("Windows Graphics Capture is not available for {kind} {id} ({e}); using snapshot capture"),
        }
    }
    #[cfg(not(windows))]
    let _ = (&restore_size, fps);
    let mut source: Box<dyn FrameSource> = if kind == "monitor" { Box::new(MonitorSource(id as usize)) } else { Box::new(WindowSource(id)) };
    let frame = source.next_frame()?;
    Ok((source, frame.width, frame.height))
}

/// Windows offered in the share picker: a named window with a size, or a minimized one whatever its
/// size (a minimized window reports a zero size).
fn shareable(window: &WindowInfo) -> bool {
    !window.title.is_empty() && (window.is_minimized || (window.width > 0 && window.height > 0))
}

/// One decimal place, as the statistics show it.
fn round1(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

#[derive(Serialize)]
pub struct MonitorDto {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

#[derive(Serialize)]
pub struct WindowDto {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    pub width: u32,
    pub height: u32,
    pub is_minimized: bool,
}

#[derive(Serialize)]
pub struct ScreenSourcesDto {
    pub monitors: Vec<MonitorDto>,
    pub windows: Vec<WindowDto>,
}

/// Monitors and windows that can be shared.
#[tauri::command]
pub async fn list_screen_sources() -> Result<ScreenSourcesDto, String> {
    tokio::task::spawn_blocking(|| {
        let monitors = list_monitors()
            .map_err(|e| format!("Could not list monitors: {e}"))?
            .into_iter()
            .map(|m| MonitorDto { id: m.id, name: m.name, width: m.width, height: m.height, is_primary: m.is_primary })
            .collect();
        // Window listing can fail on its own (e.g. no permission); monitors still work.
        let windows = list_windows()
            .unwrap_or_default()
            .into_iter()
            .filter(shareable)
            .map(|w| WindowDto { id: w.id, title: w.title, app_name: w.app_name, width: w.width, height: w.height, is_minimized: w.is_minimized })
            .collect();
        Ok(ScreenSourcesDto { monitors, windows })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Probe activation as well as the version: some Windows builds pass the version gate
/// but do not implement process loopback. Never promise exclusion on those machines.
#[tauri::command]
pub async fn process_loopback_supported() -> bool {
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(|| {
            finch_screen::process_loopback::process_loopback_supported()
                && finch_screen::process_loopback::ProcessLoopback::start(std::process::id(), false, |_| {}).is_ok()
        }).await.unwrap_or(false)
    }
    #[cfg(not(windows))]
    { false }
}

fn scope_to_source(scope: Option<&str>, kind: &str, window_pid: Option<u32>) -> AudioSourceKind {
    match (scope, kind, window_pid) {
        (Some("window"), "window", Some(pid)) => AudioSourceKind::OnlyProcess(pid),
        _ => AudioSourceKind::AllExceptSelf,
    }
}

fn requested_audio_source(scope: Option<&str>, kind: &str, window_pid: Option<u32>) -> Result<AudioSourceKind, String> {
    if scope == Some("window") && kind == "window" && window_pid.is_none() {
        return Err("window-audio-unavailable".into());
    }
    Ok(scope_to_source(scope, kind, window_pid))
}

/// Reserve before any await, including membership reads and the first frame.
async fn prepare_screen_share<T, F: std::future::Future<Output = Result<T, String>>>(
    state: &AppState,
    prepare: impl FnOnce(u64) -> F,
) -> Result<(ScreenShareReservation, T), String> {
    let reservation = state.reserve_screen_share()?;
    let prepared = prepare(reservation.generation()).await?;
    state.with_screen_share(reservation.generation(), || Ok(()))?;
    Ok((reservation, prepared))
}

/// Publish membership-dependent controls in the same order as stop/leave/logout.
async fn announce_screen_share(
    state: &AppState,
    generation: u64,
    channel_id: u32,
    session_generation: u64,
    tx: &tokio::sync::mpsc::Sender<ClientMessage>,
    announcement: ClientMessage,
    install: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _transition = state.voice_transition.lock().await;
    if *state.current_voice_channel.read().await != Some(channel_id)
        || state.session_generation.load(Ordering::SeqCst) != session_generation {
        return Err("screen-share-ended-before-start".into());
    }
    let permit = tx.reserve().await.map_err(|e| format!("Failed to announce the screen share: {e}"))?;
    state.with_screen_share(generation, || {
        permit.send(announcement);
        install()
    })
}

/// Keep the control transition locked through joining, Stop, and the end event.
/// A newer generation may reserve preflight meanwhile, but cannot publish Start.
async fn finish_screen_share(state: &AppState, generation: Option<u64>, on_stopped: impl FnOnce(u64)) -> bool {
    let _transition = state.voice_transition.lock().await;
    let tx = state.control_tx.read().await.clone();
    let ended_generation = match generation {
        Some(generation) => generation,
        None => match state.screen_share_generation() { Ok(generation) => generation, Err(_) => return false },
    };
    let stopped = match generation {
        Some(generation) => state.stop_screen_share_generation(generation),
        None => state.stop_screen_share(),
    };
    if stopped {
        if let Some(tx) = tx { let _ = tx.send(ClientMessage::StopScreenShare).await; }
        on_stopped(ended_generation);
    }
    stopped
}

/// Start sharing a monitor or window. The returned generation also tags its end
/// event, so a queued event or invoke reply cannot resurrect/hide another share.
#[tauri::command]
pub async fn start_screen_share(
    kind: String,
    id: u32,
    with_audio: Option<bool>,
    audio_scope: Option<String>,
    fps: Option<u8>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<u64, String> {
    let fps = share_fps(fps);
    let preflight_state = (*state).clone();
    let source_kind = kind.clone();
    let (reservation, (source, width, height, short_id, tx, connection, sender_peer_id, session_generation)) =
        prepare_screen_share(&state, move |generation| async move {
            let (short_id, tx, connection, sender_peer_id, session_generation) = {
                let _transition = preflight_state.voice_transition.lock().await;
                preflight_state.with_screen_share(generation, || Ok(()))?;
                let short_id = (*preflight_state.current_voice_channel.read().await)
                    .ok_or_else(|| "Join a voice channel before sharing your screen".to_string())?;
                let tx = preflight_state.control_tx.read().await.clone().ok_or_else(|| "Not connected via QUIC".to_string())?;
                let connection = preflight_state.quic_connection.read().await.clone().ok_or_else(|| "Not connected via QUIC".to_string())?;
                (short_id, tx, connection, preflight_state.session_peer_id.load(Ordering::SeqCst), preflight_state.session_generation.load(Ordering::SeqCst))
            };
            let (source, width, height) = tokio::task::spawn_blocking(move || open_source(&source_kind, id, fps))
                .await.map_err(|e| e.to_string())?
                .map_err(|e| format!("Could not capture the screen: {e}"))?;
            let (width, height) = share_size(width, height, fps);
            Ok((source, width, height, short_id, tx, connection, sender_peer_id, session_generation))
        }).await?;
    let generation = reservation.generation();
    let audio_app_handle = app_handle.clone();
    let stats_app_handle = app_handle.clone();
    let fallback_app_handle = app_handle.clone();
    let runtime = tokio::runtime::Handle::current();
    let on_end = {
        let state = (*state).clone();
        let runtime = runtime.clone();
        move |reason: String| {
            runtime.spawn(async move {
                finish_screen_share(&state, Some(generation), move |generation| {
                    let _ = app_handle.emit("screen-share-ended", serde_json::json!({ "generation": generation, "reason": reason }));
                }).await;
            });
        }
    };
    let audio_connection = connection.clone();
    announce_screen_share(&state, generation, short_id, session_generation, &tx,
        ClientMessage::StartScreenShare { codec: VideoCodec::H264, width, height, fps }, || {
            let hooks = ShareHooks {
                on_stats: Some(Box::new(move |stats: ShareStats| {
                    let _ = stats_app_handle.emit("screen-share-stats", serde_json::json!({
                        "generation": generation,
                        "captured_fps": round1(stats.captured_fps),
                        "sent_fps": round1(stats.sent_fps),
                        "encode_ms": round1(stats.encode_ms),
                        "bitrate_kbps": stats.bitrate_kbps,
                        "target_fps": stats.target_fps,
                    }));
                })),
                on_fps_fallback: Some(Box::new(move |fallback_fps: u8| {
                    let _ = fallback_app_handle.emit("screen-share-fps-fallback", serde_json::json!({ "generation": generation, "fps": fallback_fps }));
                })),
            };
            let sharer = ScreenSharer::start_with_hooks(connection, runtime, short_id, sender_peer_id,
                ShareConfig { fps, bitrate_bps: None }, source, hooks, on_end);
            *state.screen_sharer.lock().map_err(|_| "screen share state poisoned".to_string())? = Some(sharer);
            Ok(())
        }).await?;

    if with_audio.unwrap_or(false) {
        let window_pid = if kind == "window" && audio_scope.as_deref() == Some("window") {
            tokio::task::spawn_blocking(move || window_process_id(id)).await.unwrap_or(None)
        } else { None };
        let audio_source = requested_audio_source(audio_scope.as_deref(), &kind, window_pid);
        let source_app_handle = audio_app_handle.clone();
        let source_state = (*state).clone();
        let started = ScreenAudioForwarder::start(audio_connection, move || {
            let config = ScreenAudioConfig { source: audio_source?, channel_id: short_id, sender_peer_id, ..ScreenAudioConfig::default() };
            ScreenAudioCapturer::start(config).map_err(|e| e.to_string())
                .and_then(|capturer| source_state.with_screen_share(generation, || {
                    let source = match capturer.source() {
                        AudioSourceKind::AllExceptSelf => "all-except-self",
                        AudioSourceKind::OnlyProcess(_) => "only-process",
                        AudioSourceKind::Device => "device",
                    };
                    let _ = source_app_handle.emit("screen-audio-source", serde_json::json!({ "source": source }));
                    Ok(Box::new(capturer) as Box<dyn AudioPacketSource>)
                }))
        });
        match started {
            Ok(forwarder) => state.with_screen_share(generation, || {
                *state.screen_audio.lock().map_err(|_| "screen share state poisoned".to_string())? = Some(forwarder);
                Ok(())
            })?,
            Err(e) => state.with_screen_share(generation, || {
                let _ = audio_app_handle.emit("screen-audio-failed", e);
                Ok(())
            })?,
        }
    }
    state.with_screen_share(generation, || Ok(()))?;
    info!("Started sharing {kind} {id} ({width}x{height})");
    Ok(reservation.commit())
}

#[tauri::command]
pub async fn stop_screen_share(state: State<'_, AppState>) -> Result<(), String> {
    if finish_screen_share(&state, None, |_| {}).await { info!("Stopped sharing the screen"); }
    Ok(())
}

#[tauri::command]
pub async fn register_video_channel(
    on_frame: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    *state.video_channel.write().await = Some(on_frame);
    info!("Registered Tauri IPC channel for WebCodecs video frames");
    Ok(())
}

#[tauri::command]
pub async fn subscribe_screen_share(
    target_user_id: String,
    target_peer_id: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let target_uuid = Uuid::parse_str(&target_user_id)
        .map_err(|e| format!("Invalid UUID: {}", e))?;

    let tx_opt = state.control_tx.read().await.clone();
    let tx = tx_opt.ok_or_else(|| "Not connected via QUIC".to_string())?;

    // A fresh viewer: it starts by asking the sharer for a key frame.
    *state.screen_viewer.lock().await = ScreenViewer::new();
    state.viewing_peer.store(target_peer_id, Ordering::SeqCst);
    *state.viewing_user.write().await = Some(target_uuid);

    tx.send(ClientMessage::SubscribeScreenShare {
        target_user_id: target_uuid,
    })
    .await
    .map_err(|e| format!("Failed to send subscribe request: {}", e))?;

    info!("Subscribed to screen share of user {}", target_user_id);
    Ok(())
}

#[tauri::command]
pub async fn unsubscribe_screen_share(
    target_user_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let target_uuid = Uuid::parse_str(&target_user_id)
        .map_err(|e| format!("Invalid UUID: {}", e))?;
    unsubscribe_watched_share(&state, target_uuid).await
}

/// Stop watching first, so a failure to reach the server never leaves a hidden watch behind.
async fn unsubscribe_watched_share(state: &AppState, target_uuid: Uuid) -> Result<(), String> {
    state.stop_watching().await;

    // Offline there is nobody to tell: the watch is already gone here.
    let Some(tx) = state.control_tx.read().await.clone() else { return Ok(()) };

    tx.send(ClientMessage::UnsubscribeScreenShare {
        target_user_id: target_uuid,
    })
    .await
    .map_err(|e| format!("Failed to send unsubscribe request: {}", e))?;

    info!("Unsubscribed from screen share of user {}", target_uuid);
    Ok(())
}

#[cfg(test)]
mod unsubscribe_tests {
    use super::*;

    #[tokio::test]
    async fn stopping_while_offline_still_clears_the_watch() {
        let state = AppState::new();
        let alice = Uuid::new_v4();
        *state.viewing_user.write().await = Some(alice);
        state.viewing_peer.store(5, Ordering::SeqCst);
        assert!(unsubscribe_watched_share(&state, alice).await.is_ok());
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 0);
        assert!(state.viewing_user.read().await.is_none());
    }
}

#[tauri::command]
pub async fn request_idr_keyframe(
    target_peer_id: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let tx_opt = state.control_tx.read().await.clone();
    let tx = tx_opt.ok_or_else(|| "Not connected via QUIC".to_string())?;

    tx.send(ClientMessage::RequestKeyframe { target_peer_id })
        .await
        .map_err(|e| format!("Failed to send keyframe request: {}", e))?;

    info!("Requested IDR keyframe from peer {}", target_peer_id);
    Ok(())
}

static THUMBNAILS: std::sync::OnceLock<std::sync::Mutex<ThumbnailCache>> = std::sync::OnceLock::new();

/// A JPEG preview (about `max_width` px wide) of a monitor (`kind` = "monitor") or a window
/// (`kind` = "window"), for the share picker. Repeated calls within a fraction of a second
/// are answered from a cache.
#[tauri::command]
pub async fn capture_thumbnail(kind: String, id: u32, max_width: Option<u32>) -> Result<tauri::ipc::Response, String> {
    let window = match kind.as_str() {
        "monitor" => false,
        "window" => true,
        other => return Err(format!("Unknown source kind \"{other}\"")),
    };
    let width = max_width.unwrap_or(320);
    let bytes = tokio::task::spawn_blocking(move || {
        let cache = THUMBNAILS.get_or_init(|| std::sync::Mutex::new(ThumbnailCache::new()));
        let mut cache = cache.lock().map_err(|_| "preview cache is poisoned".to_string())?;
        cache.get_or_capture(window, id, std::time::Instant::now(), || {
            let frame = if window { capture_window(id) } else { capture_monitor(id as usize) }
                .map_err(|e| format!("Could not capture the preview: {e}"))?;
            thumbnail_jpeg(&frame, width)
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

#[cfg(test)]
mod share_fps_tests {
    use super::*;

    #[test]
    fn share_fps_table() {
        for (requested, expected) in [(None, 30), (Some(30), 30), (Some(60), 60), (Some(0), 30), (Some(120), 30)] {
            assert_eq!(share_fps(requested), expected, "requested {requested:?}");
        }
    }

    #[test]
    fn round1_keeps_one_decimal() {
        assert_eq!(round1(58.44), 58.4);
        assert_eq!(round1(29.96), 30.0);
    }
}

#[cfg(test)]
mod minimized_window_tests {
    use super::*;
    use finch_screen::PixelFormat;

    /// A capture session that answers its first picture with a fixed outcome.
    struct FakePicture(Option<FirstFrame>);

    impl FrameSource for FakePicture {
        fn next_frame(&mut self) -> Result<finch_screen::RawFrame, String> {
            Err("no picture".to_string())
        }
    }

    impl PictureSession for FakePicture {
        fn first_frame(&mut self, _wait: Duration) -> FirstFrame {
            self.0.take().unwrap_or(FirstFrame::NotYet)
        }
    }

    fn window(title: &str, width: u32, height: u32, is_minimized: bool) -> WindowInfo {
        WindowInfo { id: 9, title: title.into(), app_name: "game.exe".into(), width, height, is_minimized, is_maximized: false }
    }

    #[test]
    fn list_keeps_minimized_windows() {
        assert!(shareable(&window("Rocket League", 0, 0, true)));
        assert!(!shareable(&window("Rocket League", 0, 0, false)));
        assert!(!shareable(&window("", 1920, 1080, false)));
        assert!(!shareable(&window("", 0, 0, true)));
    }

    #[test]
    fn a_minimized_window_starts_without_a_first_frame() {
        let restored = start_picture_session(FakePicture(Some(FirstFrame::NotYet)), true, 9, |_| Some((800, 600)), || Some((1920, 1080)));
        assert!(matches!(restored, Some((_, 800, 600))));
        let primary = start_picture_session(FakePicture(Some(FirstFrame::NotYet)), true, 9, |_| None, || Some((1920, 1080)));
        assert!(matches!(primary, Some((_, 1920, 1080))));
    }

    #[test]
    fn a_screen_without_a_picture_or_a_closed_window_uses_snapshots() {
        assert!(start_picture_session(FakePicture(Some(FirstFrame::NotYet)), false, 1, |_| Some((800, 600)), || None).is_none());
        assert!(start_picture_session(FakePicture(Some(FirstFrame::Closed)), true, 9, |_| Some((800, 600)), || None).is_none());
    }

    #[test]
    fn a_first_picture_gives_its_size() {
        let frame = finch_screen::RawFrame::new(64, 48, PixelFormat::Bgra, bytes::Bytes::from(vec![0; 64 * 48 * 4]));
        assert!(matches!(start_picture_session(FakePicture(Some(FirstFrame::Frame(frame))), true, 9, |_| None, || None), Some((_, 64, 48))));
    }
}

#[cfg(test)]
mod audio_scope_tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stop_during_first_frame_prevents_start_announcement() {
        let state = AppState::new();
        *state.current_voice_channel.write().await = Some(7);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let worker_state = state.clone();
        let worker = tokio::spawn(async move {
            let (reservation, ()) = prepare_screen_share(&worker_state, |_| async move {
                let _ = entered_tx.send(());
                resume_rx.await.map_err(|e| e.to_string())?;
                Ok(())
            }).await?;
            announce_screen_share(&worker_state, reservation.generation(), 7, 0, &tx,
                ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 640, height: 480, fps: 30 }, || Ok(())).await
        });
        entered_rx.await.unwrap();
        let was_active = state.stop_screen_share();
        resume_tx.send(()).unwrap();
        let result = worker.await.unwrap();
        assert!(was_active);
        assert_eq!(result, Err("screen-share-ended-before-start".to_string()));
        assert!(rx.try_recv().is_err(), "a canceled preflight must not announce a new share");
        assert!(state.begin_screen_share().is_ok());
        state.stop_screen_share();
    }

    #[tokio::test]
    async fn failed_first_frame_releases_the_start_reservation() {
        let state = AppState::new();
        let result = prepare_screen_share(&state, |_| async { Err::<(), _>("capture failed".to_string()) }).await;
        assert!(matches!(result, Err(ref reason) if reason == "capture failed"));
        assert!(state.begin_screen_share().is_ok());
        state.stop_screen_share();
    }

    struct WaitingFrame {
        entered: Option<tokio::sync::oneshot::Sender<()>>,
        resume: std::sync::mpsc::Receiver<()>,
    }
    impl FrameSource for WaitingFrame {
        fn next_frame(&mut self) -> Result<finch_screen::RawFrame, String> {
            if let Some(entered) = self.entered.take() {
                let _ = entered.send(());
                self.resume.recv().map_err(|e| e.to_string())?;
            }
            Ok(finch_screen::RawFrame::new(64, 64, finch_screen::PixelFormat::Bgra, bytes::Bytes::from(vec![90; 64 * 64 * 4])))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stop_publishes_control_and_end_before_next_start() -> anyhow::Result<()> {
        let server = finch_testbed::server::TestServer::start().await?;
        let client = finch_testbed::client::TestClient::register(&server, "stopjoinrace").await?;
        let state = AppState::new();
        *state.current_voice_channel.write().await = Some(7);
        let old = state.begin_screen_share().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        *state.control_tx.write().await = Some(tx.clone());
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let sharer = ScreenSharer::start(client.session.connection.clone(), tokio::runtime::Handle::current(),
            7, client.session.session_peer_id, ShareConfig::default(),
            Box::new(WaitingFrame { entered: Some(entered_tx), resume: resume_rx }), |_| {});
        *state.screen_sharer.lock().unwrap() = Some(sharer);
        entered_rx.await.unwrap();
        let stopping_state = state.clone();
        let end_published = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped_event = end_published.clone();
        let stopping = tokio::spawn(async move {
            finish_screen_share(&stopping_state, Some(old), move |ended| {
                assert_eq!(ended, old);
                stopped_event.store(true, Ordering::SeqCst);
            }).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while state.screen_share_generation().unwrap() == old { tokio::task::yield_now().await; }
        }).await?;
        let new = state.begin_screen_share().unwrap();
        let starting_state = state.clone();
        let starting = tokio::spawn(async move {
            announce_screen_share(&starting_state, new, 7, 0, &tx,
                ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 640, height: 480, fps: 30 }, || {
                    assert!(end_published.load(Ordering::SeqCst), "publish the old end before installing new video");
                    Ok(())
                }).await
        });
        let premature = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        // Release the real capture thread even when a regression makes Start arrive too early.
        resume_tx.send(()).unwrap();
        assert!(stopping.await?);
        assert!(starting.await?.is_ok());
        assert!(premature.is_err(), "a new Start must wait while old workers are being joined");
        assert!(matches!(rx.recv().await, Some(ClientMessage::StopScreenShare)));
        assert!(matches!(rx.recv().await, Some(ClientMessage::StartScreenShare { .. })));
        state.stop_screen_share();
        Ok(())
    }

    #[tokio::test]
    async fn dropped_preflight_cancels_its_reservation() {
        let state = AppState::new();
        let worker_state = state.clone();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            prepare_screen_share(&worker_state, |_| async move {
                let _ = entered_tx.send(());
                std::future::pending::<Result<(), String>>().await
            }).await.is_ok()
        });
        entered_rx.await.unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        assert!(state.begin_screen_share().is_ok());
        state.stop_screen_share();
    }

    #[tokio::test]
    async fn a_frame_from_another_room_or_session_cannot_be_announced() {
        let state = AppState::new();
        *state.current_voice_channel.write().await = Some(7);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        for (channel, session_generation) in [(8, 0), (7, 1)] {
            let reservation = state.reserve_screen_share().unwrap();
            *state.current_voice_channel.write().await = Some(channel);
            state.session_generation.store(session_generation, Ordering::SeqCst);
            let result = announce_screen_share(&state, reservation.generation(), 7, 0, &tx,
                ClientMessage::StartScreenShare { codec: VideoCodec::H264, width: 640, height: 480, fps: 30 }, || panic!("stale frame must not install video")).await;
            assert_eq!(result, Err("screen-share-ended-before-start".to_string()));
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn missing_window_pid_disables_audio_instead_of_sharing_the_computer() {
        assert_eq!(requested_audio_source(Some("window"), "window", None), Err("window-audio-unavailable".to_string()));
        assert_eq!(requested_audio_source(Some("window"), "window", Some(7)), Ok(AudioSourceKind::OnlyProcess(7)));
    }
    #[test]
    fn scope_to_source_table() {
        for (scope, kind, pid, expected) in [
            (Some("window"), "window", Some(7), AudioSourceKind::OnlyProcess(7)),
            (Some("window"), "window", None, AudioSourceKind::AllExceptSelf),
            (Some("window"), "monitor", Some(7), AudioSourceKind::AllExceptSelf),
            (None, "window", Some(7), AudioSourceKind::AllExceptSelf),
            (Some("system"), "window", Some(7), AudioSourceKind::AllExceptSelf),
            (Some("invalid"), "window", Some(7), AudioSourceKind::AllExceptSelf),
        ] {
            assert_eq!(scope_to_source(scope, kind, pid), expected);
        }
    }
}
