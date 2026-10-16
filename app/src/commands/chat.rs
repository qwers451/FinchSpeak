//! Text messaging and channel commands.

use finch_client_core::api::ApiClient;
pub use finch_client_core::api::{ChannelInfoDto, MessagesHistoryDto};
use finch_protocol::messages::ClientMessage;
use finch_protocol::models::{AttachmentInfo, UserStatus};
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
    before_id: Option<String>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<MessagesHistoryDto, String> {
    let ch_uuid = Uuid::parse_str(&channel_id)
        .map_err(|e| format!("Invalid channel UUID: {}", e))?;
    let api = api_client(&state).await?;
    let result = match before_id {
        Some(id) => api.fetch_messages_page(ch_uuid, Some(parse_id(&id)?), limit.unwrap_or(50)).await,
        None => api.fetch_messages(ch_uuid, before, limit.unwrap_or(50)).await,
    };
    result.map_err(|e| format!("Failed to fetch messages: {}", e))
}

#[tauri::command]
pub async fn send_text_message(
    channel_id: String,
    content: String,
    attachments: Option<Vec<String>>,
    client_msg_id: Option<String>,
    reply_to: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let ch_uuid = Uuid::parse_str(&channel_id)
        .map_err(|e| format!("Invalid channel UUID: {}", e))?;
    let client_msg_id = client_msg_id
        .map(|id| Uuid::parse_str(&id).map_err(|e| format!("Invalid message id: {}", e)))
        .transpose()?;

    let reply_to = reply_to.map(|id| parse_id(&id)).transpose()?;

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
        reply_to,
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
pub async fn mark_channel_read(channel_id: String, message_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api_client(&state).await?.mark_read(parse_id(&channel_id)?, parse_id(&message_id)?).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_thumb(id: String, state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    let bytes = api_client(&state)
        .await?
        .download_thumb(parse_id(&id)?)
        .await
        .map_err(|e| format!("Preview failed: {}", e))?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
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

#[tauri::command]
pub async fn upload_avatar(data: Vec<u8>, content_type: String, state: State<'_, AppState>) -> Result<finch_client_core::api::UserDto, String> {
    let user = api_client(&state).await?.upload_avatar(data, &content_type).await.map_err(|e| e.to_string())?;
    // Keep the signed-in user the app remembers in step with the server.
    if let Some(me) = state.current_user.write().await.as_mut() {
        me.avatar_url = user.avatar_url.clone();
    }
    Ok(user)
}

#[tauri::command]
pub async fn remove_avatar(state: State<'_, AppState>) -> Result<(), String> {
    api_client(&state).await?.delete_avatar().await.map_err(|e| e.to_string())?;
    if let Some(me) = state.current_user.write().await.as_mut() {
        me.avatar_url = None;
    }
    Ok(())
}

#[tauri::command]
pub async fn download_avatar(user_id: String, state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    let bytes = api_client(&state).await?.download_avatar(parse_id(&user_id)?).await.map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}

/// Choose how you appear: `online`, `idle` or `donotdisturb`, with an optional short text.
#[tauri::command]
pub async fn set_status(status: String, text: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    let status = match status.as_str() {
        "online" => UserStatus::Online,
        "idle" => UserStatus::Idle,
        "donotdisturb" => UserStatus::DoNotDisturb,
        other => return Err(format!("Unknown status \"{other}\"")),
    };
    let tx = state.control_tx.read().await.clone().ok_or_else(|| "Not connected via QUIC Control Stream".to_string())?;
    tx.send(ClientMessage::SetStatus { status, text }).await.map_err(|e| format!("Failed to queue status: {e}"))
}
