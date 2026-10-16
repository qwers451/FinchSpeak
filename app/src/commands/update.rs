//! Looking for and installing updates (see `updater.rs` for the rules).

use std::sync::atomic::Ordering;

use tauri::{AppHandle, State};

use crate::state::AppState;
use crate::updater::{self, UpdateInfo};

/// The version of this build.
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent(concat!("FinchSpeak/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// Ask GitHub whether a newer version exists; `None` when this one is the newest.
#[tauri::command]
pub async fn check_update(state: State<'_, AppState>) -> Result<Option<UpdateInfo>, String> {
    let response = client()?.get(updater::MANIFEST_URL).send().await.map_err(|e| format!("Could not reach the update server: {e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None); // no release published yet
    }
    if !response.status().is_success() {
        return Err(format!("The update server answered {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > updater::MAX_MANIFEST_BYTES {
        return Err("The update information is too large".to_string());
    }
    let manifest: updater::Manifest = serde_json::from_slice(&bytes).map_err(|_| "The update information is not readable".to_string())?;
    let offered = updater::offer(env!("CARGO_PKG_VERSION"), &manifest);
    let info = offered.as_ref().map(|(info, _)| info.clone());
    *state.pending_update.lock().await = offered;
    Ok(info)
}

/// Download the offered update, check its signature, start its installer and quit.
#[tauri::command]
pub async fn install_update(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let (info, entry) = state.pending_update.lock().await.clone().ok_or_else(|| "There is no update to install".to_string())?;
    if !updater::download_allowed(&entry.url) {
        return Err("The update points somewhere that is not allowed".to_string());
    }
    let response = client()?.get(&entry.url).send().await.map_err(|e| format!("Download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Download failed: {}", response.status()));
    }
    if response.content_length().is_some_and(|len| len as usize > updater::MAX_INSTALLER_BYTES) {
        return Err("The installer is too large".to_string());
    }
    let bytes = response.bytes().await.map_err(|e| format!("Download failed: {e}"))?;
    if bytes.len() > updater::MAX_INSTALLER_BYTES {
        return Err("The installer is too large".to_string());
    }
    updater::verify(&bytes, &entry.signature, updater::PUBLIC_KEY_BASE64)?;

    let path = std::env::temp_dir().join(format!("FinchSpeak-update-{}.exe", info.version));
    std::fs::write(&path, &bytes).map_err(|e| format!("Could not save the installer: {e}"))?;
    std::process::Command::new(&path).spawn().map_err(|e| format!("Could not start the installer: {e}"))?;
    state.quitting.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}
