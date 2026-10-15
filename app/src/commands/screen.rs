//! Screen sharing control and WebCodecs channel registration.

use finch_protocol::messages::ClientMessage;
use tauri::State;
use tracing::info;
use uuid::Uuid;

use crate::state::{AppState, VideoChunkPayload};

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
    state: State<'_, AppState>,
) -> Result<(), String> {
    let target_uuid = Uuid::parse_str(&target_user_id)
        .map_err(|e| format!("Invalid UUID: {}", e))?;

    let tx_opt = state.control_tx.read().await.clone();
    let tx = tx_opt.ok_or_else(|| "Not connected via QUIC".to_string())?;

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
