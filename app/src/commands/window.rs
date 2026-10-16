//! Window and tray behaviour controlled from the interface.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, State};

use crate::state::AppState;
use crate::tray::{self, TrayState};

/// Closing the window hides it to the tray (`true`) or quits (`false`).
#[tauri::command]
pub async fn set_close_to_tray(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.close_to_tray.store(enabled, Ordering::SeqCst);
    Ok(())
}

/// What the tray icon shows: `idle`, `voice`, `muted` or `live`.
#[tauri::command]
pub async fn set_tray_state(state: TrayState, app: AppHandle) -> Result<(), String> {
    tray::set_state(&app, state);
    Ok(())
}

#[tauri::command]
pub async fn show_main_window(app: AppHandle) -> Result<(), String> {
    tray::show_window(&app);
    Ok(())
}
