//! Screen sharing control and WebCodecs channel registration.

use std::sync::atomic::Ordering;

use finch_client_core::screen::{encoded_size, AudioPacketSource, ScreenAudioForwarder, FrameSource, MonitorSource, ScreenSharer, ScreenViewer, ShareConfig, WindowSource};
use finch_protocol::messages::ClientMessage;
use finch_protocol::models::VideoCodec;
use finch_screen::{list_monitors, list_windows, ScreenAudioCapturer, ScreenAudioConfig};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tracing::info;
use uuid::Uuid;

use crate::state::{AppState, VideoChunkPayload};

const SHARE_FPS: u8 = 30;

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
            .filter(|w| !w.is_minimized && w.width > 0 && w.height > 0 && !w.title.is_empty())
            .map(|w| WindowDto { id: w.id, title: w.title, app_name: w.app_name, width: w.width, height: w.height })
            .collect();
        Ok(ScreenSourcesDto { monitors, windows })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Start sharing a monitor (`kind` = "monitor") or a window (`kind` = "window") in the current
/// voice channel.
#[tauri::command]
pub async fn start_screen_share(
    kind: String,
    id: u32,
    with_audio: Option<bool>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let short_id = (*state.current_voice_channel.read().await)
        .ok_or_else(|| "Join a voice channel before sharing your screen".to_string())?;
    let tx = state.control_tx.read().await.clone().ok_or_else(|| "Not connected via QUIC".to_string())?;
    let connection = state.quic_connection.read().await.clone().ok_or_else(|| "Not connected via QUIC".to_string())?;
    if state.screen_sharer.lock().map_err(|_| "screen share state poisoned".to_string())?.is_some() {
        return Err("You are already sharing your screen".into());
    }

    let mut source: Box<dyn FrameSource> = match kind.as_str() {
        "monitor" => Box::new(MonitorSource(id as usize)),
        "window" => Box::new(WindowSource(id)),
        other => return Err(format!("Unknown screen source kind: {other}")),
    };
    // Capture one frame first: it proves capture works and gives the size to announce.
    let (source, width, height) = tokio::task::spawn_blocking(move || {
        let frame = source.next_frame()?;
        Ok::<_, String>((source, frame.width, frame.height))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Could not capture the screen: {e}"))?;
    // Screens beyond 4K are scaled down before encoding; announce the size viewers will get.
    let (width, height) = encoded_size(width, height);

    tx.send(ClientMessage::StartScreenShare { codec: VideoCodec::H264, width, height, fps: SHARE_FPS })
        .await
        .map_err(|e| format!("Failed to announce the screen share: {e}"))?;

    let audio_app_handle = app_handle.clone();
    let runtime = tokio::runtime::Handle::current();
    let on_end = {
        let state = (*state).clone();
        let runtime = runtime.clone();
        move |reason: String| {
            // Runs on the capture thread: finish on the runtime (joining that thread from itself
            // would deadlock).
            runtime.spawn(async move {
                if state.stop_screen_share() {
                    if let Some(tx) = state.control_tx.read().await.clone() {
                        let _ = tx.send(ClientMessage::StopScreenShare).await;
                    }
                    let _ = app_handle.emit("screen-share-ended", reason);
                }
            });
        }
    };
    let sharer = ScreenSharer::start(
        connection,
        runtime,
        short_id,
        state.session_peer_id.load(Ordering::SeqCst),
        ShareConfig { fps: SHARE_FPS, bitrate_bps: None },
        source,
        on_end,
    );
    *state.screen_sharer.lock().map_err(|_| "screen share state poisoned".to_string())? = Some(sharer);

    if with_audio.unwrap_or(false) {
        // Best effort: the picture is already going out, so a failure here is only reported.
        let (channel_id, sender_peer_id) = (short_id, state.session_peer_id.load(Ordering::SeqCst));
        let connection = state.quic_connection.read().await.clone();
        let started = connection.map(|connection| {
            ScreenAudioForwarder::start(connection, move || {
                let config = ScreenAudioConfig { channel_id, sender_peer_id, ..ScreenAudioConfig::default() };
                ScreenAudioCapturer::start(config)
                    .map(|capturer| Box::new(capturer) as Box<dyn AudioPacketSource>)
                    .map_err(|e| e.to_string())
            })
        });
        match started {
            Some(Ok(forwarder)) => {
                if let Ok(mut slot) = state.screen_audio.lock() {
                    *slot = Some(forwarder);
                }
            }
            Some(Err(e)) => {
                let _ = audio_app_handle.emit("screen-audio-failed", e);
            }
            None => {}
        }
    }
    info!("Started sharing {kind} {id} ({width}x{height})");
    Ok(())
}

#[tauri::command]
pub async fn stop_screen_share(state: State<'_, AppState>) -> Result<(), String> {
    if state.stop_screen_share() {
        if let Some(tx) = state.control_tx.read().await.clone() {
            let _ = tx.send(ClientMessage::StopScreenShare).await;
        }
        info!("Stopped sharing the screen");
    }
    Ok(())
}

#[tauri::command]
pub async fn register_video_channel(
    on_frame: tauri::ipc::Channel<VideoChunkPayload>,
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

    let tx_opt = state.control_tx.read().await.clone();
    let tx = tx_opt.ok_or_else(|| "Not connected via QUIC".to_string())?;

    state.stop_watching().await;

    tx.send(ClientMessage::UnsubscribeScreenShare {
        target_user_id: target_uuid,
    })
    .await
    .map_err(|e| format!("Failed to send unsubscribe request: {}", e))?;

    info!("Unsubscribed from screen share of user {}", target_user_id);
    Ok(())
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
