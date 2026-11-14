//! The mini window: a small transparent, always-on-top window that shows who is talking in your room.
//! It is created on first use, hidden instead of closed, and remembers where it was dragged.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::utils::config::{BackgroundThrottlingPolicy, WindowConfig};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::state::AppState;

pub const OVERLAY_LABEL: &str = "overlay";
const POSITION_FILE: &str = "overlay_position.json";
const WIDTH: f64 = 300.0;
const HEIGHT: f64 = 360.0;

/// Top-left corner of the window in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPosition {
    pub x: i32,
    pub y: i32,
}

/// One monitor's rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

fn on_screen(screen: &Screen, x: i32, y: i32) -> bool {
    let right = screen.x as i64 + screen.width as i64;
    let bottom = screen.y as i64 + screen.height as i64;
    (x as i64) >= screen.x as i64 && (x as i64) < right && (y as i64) >= screen.y as i64 && (y as i64) < bottom
}

/// The saved spot when its corner is still on one of the monitors; `None` (default placement) otherwise,
/// for example after the monitor it was on was unplugged.
pub fn restorable(saved: Option<SavedPosition>, screens: &[Screen]) -> Option<SavedPosition> {
    let saved = saved?;
    screens.iter().any(|screen| on_screen(screen, saved.x, saved.y)).then_some(saved)
}

/// The saved position from `dir`, if the file is there and readable.
pub fn load_position(dir: &Path) -> Option<SavedPosition> {
    let text = std::fs::read_to_string(dir.join(POSITION_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_position(dir: &Path, position: SavedPosition) -> std::io::Result<()> {
    let text = serde_json::to_string(&position).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(POSITION_FILE), text)
}

/// The folder of the profile (where the pinned fingerprints and the language live).
async fn profile_dir(app: &AppHandle) -> Option<PathBuf> {
    let state = app.state::<AppState>();
    let path = state.known_servers_path.read().await.clone();
    path.and_then(|path| path.parent().map(Path::to_path_buf))
}

fn screens_of(window: &WebviewWindow) -> tauri::Result<Vec<Screen>> {
    Ok(window
        .available_monitors()?
        .iter()
        .map(|monitor| Screen {
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width,
            height: monitor.size().height,
        })
        .collect())
}

/// The settings the mini window copies from the main window's configuration.
#[derive(Debug, Default, PartialEq)]
struct InheritedSettings {
    browser_args: Option<String>,
    background_throttling: Option<BackgroundThrottlingPolicy>,
    drag_drop_disabled: bool,
}

/// Copy the browser arguments, background throttling and drag-drop setting from the `main` window.
/// Without a `main` window nothing is copied.
fn inherited_settings(windows: &[WindowConfig]) -> InheritedSettings {
    match windows.iter().find(|window| window.label == "main") {
        Some(main) => InheritedSettings {
            browser_args: main.additional_browser_args.clone(),
            background_throttling: main.background_throttling.clone(),
            drag_drop_disabled: !main.drag_drop_enabled,
        },
        None => InheritedSettings::default(),
    }
}

fn create(app: &AppHandle, dir: Option<PathBuf>) -> tauri::Result<WebviewWindow> {
    // WebView2 refuses a second webview whose browser arguments differ from the first one
    // (error 0x8007139F), so the overlay takes the main window's arguments and policies.
    let inherited = inherited_settings(&app.config().app.windows);
    let mut builder = WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App(PathBuf::from("index.html?view=overlay")));
    if let Some(args) = inherited.browser_args.as_deref() {
        builder = builder.additional_browser_args(args);
    }
    if let Some(policy) = inherited.background_throttling {
        builder = builder.background_throttling(policy);
    }
    if inherited.drag_drop_disabled {
        builder = builder.disable_drag_drop_handler();
    }
    let window = builder
        .title("FinchSpeak")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(position) => {
            if let Some(dir) = &dir {
                if let Err(e) = save_position(dir, SavedPosition { x: position.x, y: position.y }) {
                    tracing::warn!("Could not remember the mini window position: {e}");
                }
            }
        }
        // Closing only hides it: the window is reused and the tray keeps the app running.
        WindowEvent::CloseRequested { api, .. } if !handle.state::<AppState>().quitting.load(std::sync::atomic::Ordering::SeqCst) => {
            api.prevent_close();
            let _ = hide(&handle);
        }
        _ => {}
    });
    Ok(window)
}

/// Whether the mini window may be shown at all (the user's switch in the settings).
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Show the mini window, creating it on first use and putting it back where it was.
pub async fn show(app: &AppHandle) -> tauri::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let dir = profile_dir(app).await;
    let window = match app.get_webview_window(OVERLAY_LABEL) {
        Some(window) => window,
        None => create(app, dir.clone())?,
    };
    let saved = dir.as_deref().and_then(load_position);
    if let Some(position) = restorable(saved, &screens_of(&window)?) {
        window.set_position(PhysicalPosition::new(position.x, position.y))?;
    } else if saved.is_some() {
        tracing::info!("The saved mini window position is off every monitor; using the default");
    }
    window.show()?;
    app.emit("overlay-visibility", true)
}

/// Hide the mini window if it exists.
pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        window.hide()?;
    }
    app.emit("overlay-visibility", false)
}

/// Show when hidden, hide when shown. Used by the tray item and the mini-window hotkey.
pub fn toggle(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let visible = app.get_webview_window(OVERLAY_LABEL).and_then(|window| window.is_visible().ok()).unwrap_or(false);
        let result = if visible { hide(&app) } else { show(&app).await };
        if let Err(e) = result {
            tracing::warn!("Could not toggle the mini window: {e}");
        }
    });
}

/// The user's switch: off hides the window and disables the tray item; the hotkey and the game rule do nothing.
#[tauri::command]
pub async fn overlay_set_enabled(enabled: bool, app: AppHandle) -> Result<(), String> {
    ENABLED.store(enabled, Ordering::SeqCst);
    crate::tray::set_overlay_item_enabled(&app, enabled);
    if !enabled {
        hide(&app).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn overlay_show(app: AppHandle) -> Result<(), String> {
    show(&app).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn overlay_hide(app: AppHandle) -> Result<(), String> {
    hide(&app).map_err(|e| e.to_string())
}

/// Mouse clicks pass through the window to whatever is below it. Harmless when the window does not exist yet.
#[tauri::command]
pub async fn overlay_set_click_through(enabled: bool, app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        window.set_ignore_cursor_events(enabled).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(x: i32, y: i32, width: u32, height: u32) -> Screen {
        Screen { x, y, width, height }
    }

    #[test]
    fn main_window_keeps_smartscreen_and_the_mini_window_inherits_its_arguments() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let windows = config["app"]["windows"].as_array().unwrap();
        let main = windows.iter().find(|window| window["label"] == "main").unwrap();
        let args = main["additionalBrowserArgs"].as_str().unwrap();
        assert_eq!(
            args,
            "--disable-features=msWebOOUI,msPdfOOUI,CalculateNativeWinOcclusion --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows",
            "SmartScreen (msSmartScreenProtection) must stay enabled in the WebView"
        );
    }

    fn window(json: serde_json::Value) -> WindowConfig {
        serde_json::from_value(json).expect("window config")
    }

    #[test]
    fn mini_window_copies_the_main_window_browser_settings() {
        let windows = vec![
            window(serde_json::json!({
                "label": "main",
                "additionalBrowserArgs": "--main-args",
                "backgroundThrottling": "disabled",
                "dragDropEnabled": false
            })),
            window(serde_json::json!({ "label": "other", "additionalBrowserArgs": "--other-args", "dragDropEnabled": true })),
        ];
        let inherited = inherited_settings(&windows);
        assert_eq!(inherited.browser_args.as_deref(), Some("--main-args"));
        assert_eq!(inherited.background_throttling, Some(BackgroundThrottlingPolicy::Disabled));
        assert!(inherited.drag_drop_disabled);
    }

    #[test]
    fn mini_window_without_a_main_window_copies_nothing() {
        let windows = vec![window(serde_json::json!({ "label": "other", "additionalBrowserArgs": "--other-args" }))];
        assert_eq!(inherited_settings(&windows), InheritedSettings::default());
    }

    #[test]
    fn overlay_saved_position_on_a_monitor_is_restored() {
        let screens = [screen(0, 0, 1920, 1080), screen(1920, 0, 2560, 1440)];
        let inside = SavedPosition { x: 2000, y: 100 };
        assert_eq!(restorable(Some(inside), &screens), Some(inside));
        assert_eq!(restorable(Some(SavedPosition { x: 0, y: 0 }), &screens), Some(SavedPosition { x: 0, y: 0 }));
    }

    #[test]
    fn overlay_position_outside_every_monitor_is_reset() {
        let screens = [screen(0, 0, 1920, 1080)];
        assert_eq!(restorable(Some(SavedPosition { x: 5000, y: 100 }), &screens), None, "unplugged monitor to the right");
        assert_eq!(restorable(Some(SavedPosition { x: -40, y: 10 }), &screens), None, "left of the primary monitor");
        assert_eq!(restorable(Some(SavedPosition { x: 1920, y: 0 }), &screens), None, "the right edge is outside");
        assert_eq!(restorable(None, &screens), None, "nothing saved means default placement");
        assert_eq!(restorable(Some(SavedPosition { x: 0, y: 0 }), &[]), None, "no monitors at all");
    }

    #[test]
    fn overlay_position_round_trips_and_a_broken_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("finch-overlay-pos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        assert_eq!(load_position(&dir), None);
        save_position(&dir, SavedPosition { x: 412, y: -7 }).expect("save");
        assert_eq!(load_position(&dir), Some(SavedPosition { x: 412, y: -7 }));
        std::fs::write(dir.join(POSITION_FILE), "{ broken").expect("write");
        assert_eq!(load_position(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
