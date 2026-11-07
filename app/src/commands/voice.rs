//! Voice room management, audio devices, and volume controls.

use std::sync::atomic::Ordering;

use finch_audio::{list_input_devices, list_output_devices};
use finch_protocol::messages::ClientMessage;
use tauri::{AppHandle, Emitter, State};
use tracing::info;
use uuid::Uuid;

use crate::state::{AppState, AudioDeviceSelection, VoiceSettingsDto};

/// Ends our screen share for a room switch. Returns the generation that was ended, or `None`
/// when we were not sharing (then nothing is sent).
async fn end_share_for_room_change(
    state: &AppState,
    tx: &tokio::sync::mpsc::Sender<ClientMessage>,
) -> Option<u64> {
    let generation = state.screen_share_generation().ok()?;
    if !state.stop_screen_share() {
        return None;
    }
    let _ = tx.send(ClientMessage::StopScreenShare).await;
    Some(generation)
}

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
    let _transition = state.voice_transition.lock().await;

    let tx_opt = state.control_tx.read().await.clone();
    let tx = tx_opt.ok_or_else(|| "Not connected via QUIC".to_string())?;

    let switching = *state.current_voice_channel.read().await != Some(short_id);
    // Nothing is stopped until the join is sent, so a failed send leaves our share running.
    tx.send(ClientMessage::JoinVoice { channel_id: ch_uuid })
        .await
        .map_err(|e| format!("Failed to send JoinVoice: {}", e))?;

    *state.current_voice_channel.write().await = Some(short_id);
    state.mixer.lock().await.clear_playout();

    if switching {
        // Switching rooms ends our share and the share we watched there, as leaving does. The UI learns the ended generation from the event, even if the engine start below fails.
        if let Some(generation) = end_share_for_room_change(&state, &tx).await {
            let _ = app_handle.emit(
                "screen-share-ended",
                serde_json::json!({ "generation": generation, "reason": "room-changed" }),
            );
        }
        state.stop_watching().await;
    }

    // Start audio engine capture and playback
    let mut engine = state.audio_engine.lock().await;
    if let Err(e) = engine.start(short_id, (*state).clone(), app_handle) {
        return Err(format!("Failed to start audio engine: {}", e));
    }

    info!("Joined voice channel {} (short_id: {})", channel_id, short_id);
    Ok(())
}

/// Leaves the voice room: tells the server and stops our audio, share and viewing. Used by the
/// Leave command and when the app quits.
pub(crate) async fn leave_room(state: &AppState) {
    let _transition = state.voice_transition.lock().await;
    let tx_opt = state.control_tx.read().await.clone();
    if let Some(tx) = tx_opt {
        let _ = tx.send(ClientMessage::LeaveVoice).await;
    }

    state.audio_engine.lock().await.stop();
    // The server also ends the share on leave; this stops the capture thread.
    state.stop_screen_share();
    state.stop_watching().await;
    *state.current_voice_channel.write().await = None;
    state.mixer.lock().await.clear_playout();

    info!("Left voice channel");
}

#[tauri::command]
pub async fn leave_voice_channel(state: State<'_, AppState>) -> Result<(), String> {
    leave_room(state.inner()).await;
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

    let _transition = state.voice_transition.lock().await;
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

/// Spatial audio on or off: on, each voice is placed by its pan; off, plain mono.
#[tauri::command]
pub async fn set_spatial_audio(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.mixer.lock().await.set_spatial(enabled);
    Ok(())
}

/// Place a voice between left (-1) and right (1).
#[tauri::command]
pub async fn set_speaker_pan(peer_id: u32, pan: f32, state: State<'_, AppState>) -> Result<(), String> {
    state.mixer.lock().await.set_speaker_pan(peer_id, pan);
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

/// Choose the key (e.g. "F8", "Control+Space") for `action`: `push_to_talk`, `toggle_mute` or
/// `toggle_deafen`. The keys work while the app is in the background. `None` removes the key.
#[tauri::command]
pub async fn set_hotkey(
    action: String,
    key: Option<String>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let action = crate::hotkeys::HotkeyAction::parse(&action)
        .ok_or_else(|| format!("Unknown key action: {action}"))?;
    crate::hotkeys::set(&app_handle, &state, action, key.as_deref())
}

/// Record a few seconds of the processed microphone and play them back (nothing is sent to the
/// others meanwhile). Progress comes as `mic-test` events: `recording`, `playing`, `done`.
/// Outside a call the devices are opened just for the test and closed again afterwards.
#[tauri::command]
pub async fn test_microphone(
    seconds: Option<u32>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    // Serialised with join/leave, so a local test never replaces a call that just started.
    let _transition = state.voice_transition.lock().await;
    if state.is_muted.load(Ordering::SeqCst) {
        return Err("Unmute yourself first".to_string());
    }
    let in_call = state.current_voice_channel.read().await.is_some();
    let mut engine = state.audio_engine.lock().await;
    if in_call && !engine.is_running() {
        return Err("Voice audio is not running. Rejoin the voice channel to retry.".to_string());
    }
    state.mic_test_request.store(seconds.unwrap_or(3).clamp(1, 10), Ordering::SeqCst);
    if !in_call {
        engine
            .start_local_mic_test((*state).clone(), app_handle)
            .map_err(|e| format!("Failed to open the audio devices: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub fn start_audio_diagnostics(state: State<'_, AppState>) {
    state.audio_diagnostics.start();
}

#[tauri::command]
pub fn stop_audio_diagnostics(state: State<'_, AppState>) {
    state.audio_diagnostics.stop();
}

#[tauri::command]
pub fn get_audio_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    state.audio_diagnostics.report().map_err(|error| error.to_string())
}

#[cfg(test)]
mod room_change_tests {
    use super::*;

    #[tokio::test]
    async fn room_change_ends_our_share_once_and_reports_its_generation() {
        let state = AppState::new();
        let generation = state.begin_screen_share().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        assert_eq!(end_share_for_room_change(&state, &tx).await, Some(generation));
        assert!(matches!(rx.recv().await, Some(ClientMessage::StopScreenShare)));
        assert_eq!(end_share_for_room_change(&state, &tx).await, None);
        assert!(rx.try_recv().is_err(), "no second StopScreenShare when nothing was shared");
    }
}
