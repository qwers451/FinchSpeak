//! Window and tray behaviour controlled from the interface.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager, State};

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
pub async fn set_tray_state(state: TrayState, language: Option<String>, app: AppHandle, app_state: State<'_, AppState>) -> Result<(), String> {
    if let Some(language) = language.as_deref() {
        // The profile directory is where the pinned fingerprints live.
        let dir = app_state.known_servers_path.read().await.as_deref().and_then(|path| path.parent()).map(std::path::Path::to_path_buf);
        if let Some(dir) = dir {
            if tray::saved_language(&dir) != language {
                tray::save_language(&dir, language);
            }
        }
    }
    tray::set_state(&app, state, language.as_deref());
    Ok(())
}

#[tauri::command]
pub async fn show_main_window(app: AppHandle) -> Result<(), String> {
    tray::show_window(&app);
    Ok(())
}

/// Unread mentions: a red badge on the taskbar button (Windows) and a count in the tray tooltip.
#[tauri::command]
pub async fn set_attention(mentions: u32, language: Option<String>, app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        // Only Windows has overlay icons; elsewhere the error is ignored.
        let _ = window.set_overlay_icon(if mentions > 0 { Some(tray::red_badge()) } else { None });
    }
    tray::set_mentions(&app, mentions, language.as_deref());
    Ok(())
}
