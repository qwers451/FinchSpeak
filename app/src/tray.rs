//! The tray icon: opens the window, shows the call state, keeps the app running when the window is closed.

use std::sync::atomic::Ordering;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

const TRAY_ID: &str = "main";

/// What the icon says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayState {
    Idle,
    /// In a voice room, microphone open.
    Voice,
    /// In a voice room and muted.
    Muted,
    /// Sharing the screen (the window may be hidden: the red dot says so).
    Live,
}

impl TrayState {
    fn tooltip(self) -> &'static str {
        match self {
            TrayState::Idle => "FinchSpeak",
            TrayState::Voice => "FinchSpeak: in a voice room",
            TrayState::Muted => "FinchSpeak: in a voice room, muted",
            TrayState::Live => "FinchSpeak: you are sharing your screen",
        }
    }

    /// Colour of the dot in the corner of the icon, if any.
    fn dot(self) -> Option<[u8; 3]> {
        match self {
            TrayState::Idle => None,
            TrayState::Voice => Some([0x00, 0xB8, 0xBC]),
            TrayState::Muted => Some([0xFF, 0xAB, 0x00]),
            TrayState::Live => Some([0xFF, 0x33, 0x00]),
        }
    }
}

/// `rgba` (width x height) with a filled circle of `color` in the bottom-right corner.
pub fn with_dot(rgba: &[u8], width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let radius = (width.min(height) as f32 * 0.24).max(2.0);
    let (cx, cy) = (width as f32 - radius - 1.0, height as f32 - radius - 1.0);
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let distance = (dx * dx + dy * dy).sqrt();
            let index = ((y * width + x) * 4) as usize;
            if index + 3 >= out.len() {
                continue;
            }
            if distance <= radius + 1.5 && distance > radius {
                // A dark ring so the dot stands out on any taskbar.
                out[index..index + 4].copy_from_slice(&[14, 19, 32, 255]);
            } else if distance <= radius {
                out[index..index + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }
    out
}

fn icon_for(app: &AppHandle, state: TrayState) -> Option<Image<'static>> {
    let base = app.default_window_icon()?;
    let Some(color) = state.dot() else {
        return Some(base.clone().to_owned());
    };
    Some(Image::new_owned(with_dot(base.rgba(), base.width(), base.height(), color), base.width(), base.height()))
}

pub fn set_state(app: &AppHandle, state: TrayState) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Some(icon) = icon_for(app, state) {
            let _ = tray.set_icon(Some(icon));
        }
        let _ = tray.set_tooltip(Some(state.tooltip()));
    }
}

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Build the icon and its menu.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open FinchSpeak", true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", "Mute / unmute", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &mute, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(TrayState::Idle.tooltip())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "mute" => {
                let _ = app.emit("tray-toggle-mute", ());
            }
            "quit" => {
                app.state::<AppState>().quitting.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dot_lands_in_the_bottom_right_corner_and_only_there() {
        let base = vec![10u8; 32 * 32 * 4];
        let marked = with_dot(&base, 32, 32, [255, 51, 0]);
        let pixel = |x: usize, y: usize| &marked[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(pixel(32 - 5, 32 - 5), &[255, 51, 0, 255], "centre of the dot");
        assert_eq!(pixel(2, 2), &[10, 10, 10, 10], "the rest is untouched");
        assert_eq!(marked.len(), base.len());
    }

    #[test]
    fn tiny_or_odd_sizes_do_not_panic() {
        for (w, h) in [(1, 1), (2, 3), (16, 16)] {
            let _ = with_dot(&vec![0u8; (w * h * 4) as usize], w, h, [1, 2, 3]);
        }
        let _ = with_dot(&[0u8; 7], 4, 4, [1, 2, 3]);
    }

    #[test]
    fn every_state_has_a_tooltip_and_only_idle_has_no_dot() {
        for state in [TrayState::Idle, TrayState::Voice, TrayState::Muted, TrayState::Live] {
            assert!(state.tooltip().starts_with("FinchSpeak"));
            assert_eq!(state.dot().is_none(), state == TrayState::Idle);
        }
    }
}
