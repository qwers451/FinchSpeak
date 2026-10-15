//! Voice room management, audio devices, and volume controls.

use std::sync::atomic::Ordering;

use finch_audio::{list_input_devices, list_output_devices};
use finch_protocol::messages::ClientMessage;
use tauri::{AppHandle, State};
use tracing::info;
use uuid::Uuid;

use crate::state::AppState;

#[derive(serde::Serialize)]
pub struct AudioDeviceDto {
    pub name: String,
    pub is_default: bool,
}

#[derive(serde::Serialize)]
pub struct AudioDevicesDto {
    pub inputs: Vec<AudioDeviceDto>,
    pub outputs: Vec<AudioDeviceDto>,
}

#[tauri::command]
pub async fn join_voice_channel(
    channel_id: String,
    short_id: u32,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let ch_uuid = Uuid::parse_str(&channel_id)
        .map_err(|e| format!("Invalid channel UUID: {}", e))?;

    let tx_opt = state.control_tx.read().await.clone();
    let tx = tx_opt.ok_or_else(|| "Not connected via QUIC".to_string())?;

    tx.send(ClientMessage::JoinVoice { channel_id: ch_uuid })
        .await
        .map_err(|e| format!("Failed to send JoinVoice: {}", e))?;

    *state.current_voice_channel.write().await = Some(short_id);

    // Start audio engine capture and playback
    let mut engine = state.audio_engine.lock().await;
    if let Err(e) = engine.start(short_id, (*state).clone(), app_handle) {
        return Err(format!("Failed to start audio engine: {}", e));
    }

    info!("Joined voice channel {} (short_id: {})", channel_id, short_id);
    Ok(())
}

#[tauri::command]
pub async fn leave_voice_channel(state: State<'_, AppState>) -> Result<(), String> {
    let tx_opt = state.control_tx.read().await.clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(ClientMessage::LeaveVoice).await;
    }

    state.audio_engine.lock().await.stop();
    *state.current_voice_channel.write().await = None;

    info!("Left voice channel");
    Ok(())
}

#[tauri::command]
pub async fn set_voice_state(
    muted: bool,
    deafened: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.is_muted.store(muted, Ordering::SeqCst);
    state.is_deafened.store(deafened, Ordering::SeqCst);

    let tx_opt = state.control_tx.read().await.clone();
    if let Some(tx) = tx_opt {
        let _ = tx
            .send(ClientMessage::UpdateVoiceState { muted, deafened })
            .await;
    }

    info!("Voice state updated: muted={}, deafened={}", muted, deafened);
    Ok(())
}

#[tauri::command]
pub async fn get_audio_devices() -> Result<AudioDevicesDto, String> {
    let inputs = list_input_devices()
        .map_err(|e| format!("Failed to query inputs: {}", e))?
        .into_iter()
        .map(|d| AudioDeviceDto {
            name: d.name,
            is_default: d.is_default,
        })
        .collect();

    let outputs = list_output_devices()
        .map_err(|e| format!("Failed to query outputs: {}", e))?
        .into_iter()
        .map(|d| AudioDeviceDto {
            name: d.name,
            is_default: d.is_default,
        })
        .collect();

    Ok(AudioDevicesDto { inputs, outputs })
}

#[tauri::command]
pub async fn set_speaker_volume(
    peer_id: u32,
    volume: f32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut mixer = state.mixer.lock().await;
    mixer.set_speaker_volume(peer_id, volume);
    Ok(())
}
