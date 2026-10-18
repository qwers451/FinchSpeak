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

struct TrayLabels {
    open: MenuItem<tauri::Wry>,
    mute: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

struct TrayCopy {
    open: &'static str,
    mute: &'static str,
    quit: &'static str,
}

fn tray_copy(language: &str) -> TrayCopy {
    if language == "en" {
        TrayCopy { open: "Open FinchSpeak", mute: "Mute / unmute", quit: "Quit" }
    } else {
        TrayCopy { open: "Открыть FinchSpeak", mute: "Включить / выключить микрофон", quit: "Выйти" }
    }
}

impl TrayState {
    fn tooltip(self, language: &str) -> &'static str {
        if language != "en" { return match self {
            TrayState::Idle => "FinchSpeak",
            TrayState::Voice => "FinchSpeak: в голосовой комнате",
            TrayState::Muted => "FinchSpeak: микрофон выключен",
            TrayState::Live => "FinchSpeak: идёт демонстрация экрана",
        }; }
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

fn bird_icon_bytes(state: TrayState) -> &'static [u8] {
    match state {
        TrayState::Idle => include_bytes!("../frontend/public/custom/assets/tray-icons/png/tray-01-online-32@2x.png"),
        TrayState::Voice | TrayState::Live => include_bytes!("../frontend/public/custom/assets/tray-icons/png/tray-02-voice-32@2x.png"),
        TrayState::Muted => include_bytes!("../frontend/public/custom/assets/tray-icons/png/tray-04-muted-32@2x.png"),
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
    let base = Image::from_bytes(bird_icon_bytes(state)).ok().or_else(|| app.default_window_icon().cloned())?.to_owned();
    let Some(color) = state.dot() else {
        return Some(base);
    };
    Some(Image::new_owned(with_dot(base.rgba(), base.width(), base.height(), color), base.width(), base.height()))
}

pub fn set_state(app: &AppHandle, state: TrayState, language: Option<&str>) {
    let language = language.unwrap_or("ru");
    let copy = tray_copy(language);
    if let Some(labels) = app.try_state::<TrayLabels>() {
        let _ = labels.open.set_text(copy.open);
        let _ = labels.mute.set_text(copy.mute);
        let _ = labels.quit.set_text(copy.quit);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Some(icon) = icon_for(app, state) {
            let _ = tray.set_icon(Some(icon));
        }
        let _ = tray.set_tooltip(Some(state.tooltip(language)));
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
    let copy = tray_copy("ru");
    let open = MenuItem::with_id(app, "open", copy.open, true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", copy.mute, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", copy.quit, true, None::<&str>)?;
    app.manage(TrayLabels { open: open.clone(), mute: mute.clone(), quit: quit.clone() });
    let menu = Menu::with_items(app, &[&open, &mute, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(TrayState::Idle.tooltip("ru"))
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
    if let Some(icon) = icon_for(app, TrayState::Idle) {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_tray_copy_supports_russian_english_and_safe_default() {
        let russian = tray_copy("ru");
        let english = tray_copy("en");
        assert_eq!(russian.open, "Открыть FinchSpeak");
        assert_eq!(russian.mute, "Включить / выключить микрофон");
        assert_eq!(english.open, "Open FinchSpeak");
        assert_eq!(english.quit, "Quit");
        assert_eq!(tray_copy("invalid").quit, russian.quit);
        assert_ne!(russian.quit, english.quit);
    }

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
            assert!(state.tooltip("en").starts_with("FinchSpeak"));
            assert_eq!(state.dot().is_none(), state == TrayState::Idle);
        }
    }

    #[test]
    fn tray_states_use_the_supplied_bird_artwork() {
        let idle = Image::from_bytes(bird_icon_bytes(TrayState::Idle)).unwrap();
        let voice = Image::from_bytes(bird_icon_bytes(TrayState::Voice)).unwrap();
        let muted = Image::from_bytes(bird_icon_bytes(TrayState::Muted)).unwrap();
        assert_eq!((idle.width(), idle.height()), (64, 64));
        assert_eq!((voice.width(), voice.height()), (64, 64));
        assert_eq!((muted.width(), muted.height()), (64, 64));
        assert_ne!(idle.rgba(), voice.rgba());
        assert_ne!(voice.rgba(), muted.rgba());
    }
}
