//! Voice room management, audio devices, and volume controls.

use std::sync::atomic::Ordering;

use finch_audio::{list_input_devices, list_output_devices};
use finch_protocol::messages::ClientMessage;
use tauri::{AppHandle, State};
use tracing::info;
use uuid::Uuid;

use crate::state::{AppState, AudioDeviceSelection, VoiceSettingsDto};

#[derive(serde::Serialize)]
pub struct AudioDeviceDto {
    pub name: String,
    pub is_default: bool,
}

#[derive(serde::Serialize)]
pub struct AudioDevicesDto {
    pub inputs: Vec<AudioDeviceDto>,
    pub outputs: Vec<AudioDeviceDto>,
    /// What the user picked; `null` = system default.
    pub selected_input: Option<String>,
    pub selected_output: Option<String>,
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
    // The server also ends the share on leave; this stops the capture thread.
    state.stop_screen_share();
    state.stop_watching().await;
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
    state.voice_settings.set_muted(muted);
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
pub async fn get_audio_devices(state: State<'_, AppState>) -> Result<AudioDevicesDto, String> {
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

    let selection = state.audio_devices.read().map(|s| s.clone()).unwrap_or_default();
    Ok(AudioDevicesDto {
        inputs,
        outputs,
        selected_input: selection.input,
        selected_output: selection.output,
    })
}

/// Choose the microphone and speakers (`None` = system default). Takes effect immediately: if
/// the user is in a voice channel the audio streams are reopened on the new devices.
#[tauri::command]
pub async fn set_audio_devices(
    input: Option<String>,
    output: Option<String>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let selection = AudioDeviceSelection::new(input, output);
    {
        let mut current = state
            .audio_devices
            .write()
            .map_err(|_| "Audio device settings are unavailable".to_string())?;
        if *current == selection {
            return Ok(());
        }
        *current = selection;
    }

    let channel = *state.current_voice_channel.read().await;
    if let Some(short_id) = channel {
        state
            .audio_engine
            .lock()
            .await
            .start(short_id, (*state).clone(), app_handle)
            .map_err(|e| format!("Failed to reopen audio devices: {}", e))?;
    }
    Ok(())
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

#[tauri::command]
pub async fn get_voice_settings(state: State<'_, AppState>) -> Result<VoiceSettingsDto, String> {
    Ok(state.voice_settings.snapshot())
}

/// Microphone volume, speech threshold, voice enhancement, push-to-talk and monitoring.
#[tauri::command]
pub async fn set_voice_settings(settings: VoiceSettingsDto, state: State<'_, AppState>) -> Result<(), String> {
    state.voice_settings.apply(&settings);
    Ok(())
}

/// Choose the push-to-talk key (e.g. "F8", "Control+Space"); works while the app is in the
/// background. Passing `None` removes the key.
#[tauri::command]
pub async fn set_push_to_talk_key(key: Option<String>, app_handle: AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let shortcuts = app_handle.global_shortcut();
    shortcuts.unregister_all().map_err(|e| format!("Could not clear the old key: {e}"))?;
    if let Some(key) = key.filter(|k| !k.trim().is_empty()) {
        shortcuts.register(key.as_str()).map_err(|e| format!("Could not use \"{key}\" as the push-to-talk key: {e}"))?;
    }
    Ok(())
}

/// Record a few seconds of the processed microphone and play them back (nothing is sent to the
/// others meanwhile). Progress comes as `mic-test` events: `recording`, `playing`, `done`.
#[tauri::command]
pub async fn test_microphone(seconds: Option<u32>, state: State<'_, AppState>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if !state.audio_engine.lock().await.is_running() {
        return Err("Join a voice channel first: the microphone is only open during a call".to_string());
    }
    if state.is_muted.load(Ordering::SeqCst) {
        return Err("Unmute yourself first".to_string());
    }
    state.mic_test_request.store(seconds.unwrap_or(3).clamp(1, 10), Ordering::SeqCst);
    Ok(())
}
