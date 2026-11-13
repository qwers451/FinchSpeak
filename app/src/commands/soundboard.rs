//! Soundboard commands: the board is listed, uploaded to and removed through the REST API; a play is one
//! control-stream message to my voice room. Sounds are fetched once and played by each client from its own cache.

use finch_client_core::error::ClientError;
use finch_protocol::messages::ClientMessage;
use finch_protocol::models::SoundInfo;
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

fn parse_id(id: &str) -> Result<Uuid, String> {
    Uuid::parse_str(id).map_err(|e| format!("Invalid id: {}", e))
}

async fn api_client(state: &AppState) -> Result<finch_client_core::api::ApiClient, String> {
    state.api.read().await.clone().ok_or_else(|| "Not authenticated".to_string())
}

/// The sounds on the board, in the order they were added.
#[tauri::command]
pub async fn soundboard(state: State<'_, AppState>) -> Result<Vec<SoundInfo>, String> {
    api_client(&state).await?.soundboard().await.map_err(|e| e.to_string())
}

/// Add a sound. `data` is a 16-bit mono 24 kHz WAV of at most 5 seconds (the frontend renders it).
#[tauri::command]
pub async fn upload_board_sound(data: Vec<u8>, name: String, emoji: Option<String>, state: State<'_, AppState>) -> Result<SoundInfo, String> {
    api_client(&state).await?.upload_board_sound(data, &name, emoji.as_deref()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_board_sound(sound_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api_client(&state).await?.delete_board_sound(parse_id(&sound_id)?).await.map_err(|e| e.to_string())
}

/// A sound's WAV bytes; an empty response means the sound is gone.
#[tauri::command]
pub async fn download_board_sound(sound_id: String, state: State<'_, AppState>) -> Result<tauri::ipc::Response, String> {
    match api_client(&state).await?.download_board_sound_cached(parse_id(&sound_id)?, None).await {
        Ok((bytes, _)) => Ok(tauri::ipc::Response::new(bytes.map(|b| b.to_vec()).unwrap_or_default())),
        Err(ClientError::Http { status: 404, .. }) => Ok(tauri::ipc::Response::new(Vec::<u8>::new())),
        Err(e) => Err(e.to_string()),
    }
}

/// Play a sound for everybody in my voice room (me included).
#[tauri::command]
pub async fn play_board_sound(sound_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let sound_id = parse_id(&sound_id)?;
    if state.current_voice_channel.read().await.is_none() {
        return Err("Not in a voice channel".into());
    }
    let sender = state.control_tx.read().await.clone().ok_or("Not connected via QUIC")?;
    sender.send(ClientMessage::PlaySound { sound_id }).await.map_err(|e| e.to_string())
}
