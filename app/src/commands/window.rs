//! Window and tray behaviour controlled from the interface.

use tauri::{AppHandle, Manager, State};

use crate::notify::{self, Notice};
use crate::state::AppState;
use crate::tray::{self, TrayState};

/// A Windows notification with the sender's picture (PNG bytes, optional) and buttons. Other
/// systems get an error and use the notification plugin instead.
#[tauri::command]
pub async fn notify(notice: Notice, avatar_png: Option<Vec<u8>>, app: AppHandle) -> Result<(), String> {
    let cache_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || notify::send(&app, notice, avatar_png, cache_dir))
        .await
        .map_err(|e| e.to_string())?
}

/// The invite link that started the app (cold start), handed over once.
#[tauri::command]
pub fn take_invite_link() -> Option<String> {
    crate::take_pending_invite()
}

/// What the tray icon shows: `idle`, `voice`, `speaking`, `muted` or `live`.
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

/// Seconds since the last keyboard or mouse input on this computer; `0` when it cannot be read.
#[tauri::command]
pub fn system_idle_seconds() -> u64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetTickCount;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        // SAFETY: `info` is a valid LASTINPUTINFO whose cbSize is set, as GetLastInputInfo requires.
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return 0;
        }
        // SAFETY: GetTickCount has no preconditions.
        let now = unsafe { GetTickCount() };
        // The tick count wraps every ~49 days, so the difference must wrap too.
        u64::from(now.wrapping_sub(info.dwTime) / 1000)
    }
    #[cfg(not(windows))]
    {
        0
    }
}
