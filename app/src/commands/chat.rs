//! Text messaging and channel commands.

use finch_protocol::messages::ClientMessage;
use finch_protocol::models::{AttachmentInfo, HistoryMessage};
use reqwest::multipart::{Form, Part};
use tauri::State;
use tracing::info;
use uuid::Uuid;

use crate::state::AppState;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ChannelInfoDto {
    pub id: String,
    pub short_id: u32,
    pub name: String,
    pub channel_type: String,
    pub topic: Option<String>,
    pub position: i32,
    pub bitrate_kbps: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MessagesHistoryDto {
    pub channel_id: String,
    pub messages: Vec<HistoryMessage>,
    pub has_more: bool,
}

#[tauri::command]
pub async fn list_channels(state: State<'_, AppState>) -> Result<Vec<ChannelInfoDto>, String> {
    let server_url = state
        .server_url
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not connected to any server".to_string())?;

    let token = state
        .auth_token
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let url = format!("{}/api/channels", server_url.trim_end_matches('/'));
    let resp = state
        .http_client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch channels: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Server returned error: {}", resp.status()));
    }

    let channels: Vec<ChannelInfoDto> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse channels: {}", e))?;

    Ok(channels)
}

#[tauri::command]
pub async fn create_channel(
    name: String,
    channel_type: String,
    topic: Option<String>,
    bitrate_kbps: Option<u32>,
    state: State<'_, AppState>,
) -> Result<ChannelInfoDto, String> {
    let server_url = state
        .server_url
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not connected".to_string())?;

    let token = state
        .auth_token
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let url = format!("{}/api/channels", server_url.trim_end_matches('/'));
    let resp = state
        .http_client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "name": name,
            "channel_type": channel_type,
            "topic": topic,
            "bitrate_kbps": bitrate_kbps
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to create channel: {}", e))?;

    if !resp.status().is_success() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to create channel: {}", err));
    }

    let channel: ChannelInfoDto = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse channel response: {}", e))?;

    Ok(channel)
}

#[tauri::command]
pub async fn fetch_channel_messages(
    channel_id: String,
    before: Option<i64>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<MessagesHistoryDto, String> {
    let server_url = state
        .server_url
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not connected".to_string())?;

    let token = state
        .auth_token
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let mut url = format!(
        "{}/api/channels/{}/messages?limit={}",
        server_url.trim_end_matches('/'),
        channel_id,
        limit.unwrap_or(50)
    );
    if let Some(b) = before {
        url.push_str(&format!("&before={}", b));
    }

    let resp = state
        .http_client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch messages: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Failed to fetch messages: {}", resp.status()));
    }

    let history: MessagesHistoryDto = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse messages history: {}", e))?;

    Ok(history)
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
    let server_url = state
        .server_url
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not connected".to_string())?;

    let token = state
        .auth_token
        .read()
        .await
        .clone()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let url = format!("{}/api/files/upload", server_url.trim_end_matches('/'));

    let part = Part::bytes(data)
        .file_name(filename)
        .mime_str(&content_type)
        .map_err(|e| format!("Invalid mime type: {}", e))?;

    let form = Form::new().part("file", part);

    let resp = state
        .http_client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("File upload request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("File upload failed: {}", resp.status()));
    }

    let attachment: AttachmentInfo = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse upload response: {}", e))?;

    Ok(attachment)
}
