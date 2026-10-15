//! Text messaging and channel commands.

use finch_client_core::api::ApiClient;
pub use finch_client_core::api::{ChannelInfoDto, MessagesHistoryDto};
use finch_protocol::messages::ClientMessage;
use finch_protocol::models::AttachmentInfo;
use tauri::State;
use tracing::info;
use uuid::Uuid;

use crate::state::AppState;

async fn api_client(state: &AppState) -> Result<ApiClient, String> {
    state
        .api
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not authenticated".to_string())
}

#[tauri::command]
pub async fn list_channels(state: State<'_, AppState>) -> Result<Vec<ChannelInfoDto>, String> {
    api_client(&state)
        .await?
        .list_channels()
        .await
        .map_err(|e| format!("Failed to fetch channels: {}", e))
}

#[tauri::command]
pub async fn create_channel(
    name: String,
    channel_type: String,
    topic: Option<String>,
    bitrate_kbps: Option<u32>,
    state: State<'_, AppState>,
) -> Result<ChannelInfoDto, String> {
    api_client(&state)
        .await?
        .create_channel(&name, &channel_type, topic, bitrate_kbps)
        .await
        .map_err(|e| format!("Failed to create channel: {}", e))
}

#[tauri::command]
pub async fn fetch_channel_messages(
    channel_id: String,
    before: Option<i64>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<MessagesHistoryDto, String> {
    let ch_uuid = Uuid::parse_str(&channel_id)
        .map_err(|e| format!("Invalid channel UUID: {}", e))?;
    api_client(&state)
        .await?
        .fetch_messages(ch_uuid, before, limit.unwrap_or(50))
        .await
        .map_err(|e| format!("Failed to fetch messages: {}", e))
}

#[tauri::command]
pub async fn send_text_message(
    channel_id: String,
    content: String,
    attachments: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let ch_uuid = Uuid::parse_str(&channel_id)
        .map_err(|e| format!("Invalid channel UUID: {}", e))?;

    let attach_uuids: Vec<Uuid> = attachments
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| Uuid::parse_str(&s).ok())
        .collect();

    let tx_opt = state.control_tx.read().await.clone();
    let tx = tx_opt.ok_or_else(|| "Not connected via QUIC Control Stream".to_string())?;

    tx.send(ClientMessage::SendMessage {
        channel_id: ch_uuid,
        content,
        attachments: attach_uuids,
    })
    .await
    .map_err(|e| format!("Failed to queue message: {}", e))?;

    info!("Message sent to channel {}", channel_id);
    Ok(())
}

#[tauri::command]
pub async fn upload_file(
    filename: String,
    content_type: String,
    data: Vec<u8>,
    state: State<'_, AppState>,
) -> Result<AttachmentInfo, String> {
    api_client(&state)
        .await?
        .upload_file(&filename, data, &content_type)
        .await
        .map_err(|e| format!("File upload failed: {}", e))
}
