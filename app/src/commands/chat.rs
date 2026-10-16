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
    client_msg_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let ch_uuid = Uuid::parse_str(&channel_id)
        .map_err(|e| format!("Invalid channel UUID: {}", e))?;
    let client_msg_id = client_msg_id
        .map(|id| Uuid::parse_str(&id).map_err(|e| format!("Invalid message id: {}", e)))
        .transpose()?;

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
        client_msg_id,
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

#[tauri::command]
pub async fn download_file(id: String, state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    let id = Uuid::parse_str(&id).map_err(|e| format!("Invalid file id: {}", e))?;
    let bytes = api_client(&state)
        .await?
        .download_file(id)
        .await
        .map_err(|e| format!("Download failed: {}", e))?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}

fn parse_id(id: &str) -> Result<Uuid, String> {
    Uuid::parse_str(id).map_err(|e| format!("Invalid id: {}", e))
}

#[tauri::command]
pub async fn edit_message(message_id: String, content: String, state: State<'_, AppState>) -> Result<(), String> {
    api_client(&state).await?.edit_message(parse_id(&message_id)?, &content).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_message(message_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api_client(&state).await?.delete_message(parse_id(&message_id)?).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_reaction(message_id: String, emoji: String, on: bool, state: State<'_, AppState>) -> Result<(), String> {
    let api = api_client(&state).await?;
    let id = parse_id(&message_id)?;
    let result = if on { api.add_reaction(id, &emoji).await } else { api.remove_reaction(id, &emoji).await };
    result.map_err(|e| e.to_string())
}

/// Tell the others we are typing in a channel (the server throttles it).
#[tauri::command]
pub async fn send_typing(channel_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let channel_id = parse_id(&channel_id)?;
    let tx = state.control_tx.read().await.clone().ok_or_else(|| "Not connected".to_string())?;
    tx.send(ClientMessage::Typing { channel_id }).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_profile(display_name: String, state: State<'_, AppState>) -> Result<finch_client_core::api::UserDto, String> {
    let user = api_client(&state).await?.update_profile(&display_name).await.map_err(|e| e.to_string())?;
    *state.current_user.write().await = Some(user.clone());
    Ok(user)
}

#[tauri::command]
pub async fn change_password(current_password: String, new_password: String, state: State<'_, AppState>) -> Result<(), String> {
    api_client(&state)
        .await?
        .change_password(&current_password, &new_password)
        .await
        .map_err(|e| e.to_string())
}
