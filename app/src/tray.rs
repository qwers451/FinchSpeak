//! The tray icon: opens the window, shows the call state, keeps the app running when the window is closed.

use std::path::Path;
use std::sync::atomic::Ordering;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

const TRAY_ID: &str = "main";
const LANGUAGE_FILE: &str = "ui_language.txt";

/// The last state and mention count shown, so each update can rebuild the whole tooltip.
static SHOWN: std::sync::Mutex<(TrayState, u32)> = std::sync::Mutex::new((TrayState::Idle, 0));

/// What the icon says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayState {
    Idle,
    /// In a voice room, microphone open.
    Voice,
    /// Speaking in a voice room.
    Speaking,
    /// In a voice room and muted.
    Muted,
    /// Sharing the screen (the window may be hidden: the red dot says so).
    Live,
}

struct TrayLabels {
    open: MenuItem<tauri::Wry>,
    overlay: MenuItem<tauri::Wry>,
    mute: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

struct TrayCopy {
    open: &'static str,
    overlay: &'static str,
    mute: &'static str,
    quit: &'static str,
}

fn tray_copy(language: &str) -> TrayCopy {
    if language == "en" {
        TrayCopy { open: "Open FinchSpeak", overlay: "Mini window", mute: "Mute / unmute", quit: "Quit" }
    } else {
        TrayCopy { open: "Открыть FinchSpeak", overlay: "Мини-окно", mute: "Включить / выключить микрофон", quit: "Выйти" }
    }
}

impl TrayState {
    fn tooltip(self, language: &str) -> &'static str {
        if language != "en" { return match self {
            TrayState::Idle => "FinchSpeak",
            TrayState::Voice => "FinchSpeak: в голосовой комнате",
            TrayState::Speaking => "FinchSpeak: вы говорите",
            TrayState::Muted => "FinchSpeak: микрофон выключен",
            TrayState::Live => "FinchSpeak: идёт демонстрация экрана",
        }; }
        match self {
            TrayState::Idle => "FinchSpeak",
            TrayState::Voice => "FinchSpeak: in a voice room",
            TrayState::Speaking => "FinchSpeak: you are speaking",
            TrayState::Muted => "FinchSpeak: in a voice room, muted",
            TrayState::Live => "FinchSpeak: you are sharing your screen",
        }
    }

    /// Colour of the dot in the corner of the icon, if any.
    fn dot(self) -> Option<[u8; 3]> {
        match self {
            TrayState::Idle => None,
            TrayState::Voice => Some([0x00, 0xB8, 0xBC]),
            TrayState::Speaking => Some([0x2E, 0xCC, 0x71]),
            TrayState::Muted => Some([0xFF, 0xAB, 0x00]),
            TrayState::Live => Some([0xFF, 0x33, 0x00]),
        }
    }
}

/// One tray picture for every state; the state is shown by the coloured dot.
const TRAY_PICTURE: &[u8] = include_bytes!("../icons/tray.png");

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

/// A 16x16 transparent image with a red dot: the taskbar overlay for unread mentions.
pub fn red_badge() -> Image<'static> {
    let blank = vec![0u8; 16 * 16 * 4];
    Image::new_owned(with_dot(&blank, 16, 16, [0xF2, 0x3F, 0x43]), 16, 16)
}

/// "упоминание" / "упоминания" / "упоминаний" for `n`.
fn russian_mentions(n: u32) -> &'static str {
    match (n % 10, n % 100) {
        (_, 11..=14) => "упоминаний",
        (1, _) => "упоминание",
        (2..=4, _) => "упоминания",
        _ => "упоминаний",
    }
}

/// `base` plus " · {n} упоминаний" / " · {n} mentions" when there are any.
fn tooltip_with_mentions(base: &str, mentions: u32, language: &str) -> String {
    if mentions == 0 {
        return base.to_string();
    }
    if language == "en" {
        format!("{base} · {mentions} {}", if mentions == 1 { "mention" } else { "mentions" })
    } else {
        format!("{base} · {mentions} {}", russian_mentions(mentions))
    }
}

/// The tray picture with the dot for `state` (no dot when idle).
fn state_icon(base: &Image<'static>, state: TrayState) -> Image<'static> {
    match state.dot() {
        None => base.to_owned(),
        Some(color) => Image::new_owned(with_dot(base.rgba(), base.width(), base.height(), color), base.width(), base.height()),
    }
}

fn icon_for(app: &AppHandle, state: TrayState) -> Option<Image<'static>> {
    let base = Image::from_bytes(TRAY_PICTURE).ok().or_else(|| app.default_window_icon().cloned())?.to_owned();
    Some(state_icon(&base, state))
}

pub fn set_state(app: &AppHandle, state: TrayState, language: Option<&str>) {
    let language = language.unwrap_or("ru");
    let mentions = {
        let mut shown = SHOWN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        shown.0 = state;
        shown.1
    };
    apply(app, state, mentions, language);
}

/// Update the mention count in the tray tooltip (the icon stays as it is).
pub fn set_mentions(app: &AppHandle, mentions: u32, language: Option<&str>) {
    let state = {
        let mut shown = SHOWN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        shown.1 = mentions;
        shown.0
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip_with_mentions(state.tooltip(language.unwrap_or("ru")), mentions, language.unwrap_or("ru"))));
    }
}

fn apply(app: &AppHandle, state: TrayState, mentions: u32, language: &str) {
    let copy = tray_copy(language);
    if let Some(labels) = app.try_state::<TrayLabels>() {
        let _ = labels.open.set_text(copy.open);
        let _ = labels.overlay.set_text(copy.overlay);
        let _ = labels.mute.set_text(copy.mute);
        let _ = labels.quit.set_text(copy.quit);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Some(icon) = icon_for(app, state) {
            let _ = tray.set_icon(Some(icon));
        }
        let _ = tray.set_tooltip(Some(tooltip_with_mentions(state.tooltip(language), mentions, language)));
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
/// The interface language saved by the last session; anything but `en` reads as `ru`.
pub fn saved_language(dir: &Path) -> String {
    match std::fs::read_to_string(dir.join(LANGUAGE_FILE)) {
        Ok(text) if text.trim() == "en" => "en".to_string(),
        _ => "ru".to_string(),
    }
}

/// Remember the interface language so the tray menu starts in it next time.
pub fn save_language(dir: &Path, language: &str) {
    let language = if language == "en" { "en" } else { "ru" };
    if let Err(e) = std::fs::write(dir.join(LANGUAGE_FILE), language) {
        tracing::warn!("Could not save the interface language: {e}");
    }
}

pub fn setup(app: &AppHandle, language: &str) -> tauri::Result<()> {
    let copy = tray_copy(language);
    let open = MenuItem::with_id(app, "open", copy.open, true, None::<&str>)?;
    let overlay = MenuItem::with_id(app, "overlay", copy.overlay, true, None::<&str>)?;
    let mute = MenuItem::with_id(app, "mute", copy.mute, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", copy.quit, true, None::<&str>)?;
    app.manage(TrayLabels { open: open.clone(), overlay: overlay.clone(), mute: mute.clone(), quit: quit.clone() });
    let menu = Menu::with_items(app, &[&open, &overlay, &mute, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(TrayState::Idle.tooltip(language))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_window(app),
            "overlay" => crate::overlay::toggle_from_tray(app),
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
    fn language_round_trip_defaults_to_ru() {
        let dir = std::env::temp_dir().join(format!("finch-tray-lang-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(dir.join(LANGUAGE_FILE));
        assert_eq!(saved_language(&dir), "ru");
        save_language(&dir, "en");
        assert_eq!(saved_language(&dir), "en");
        save_language(&dir, "xx");
        assert_eq!(saved_language(&dir), "ru");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
    fn badge_is_16px_with_opaque_centre() {
        let badge = red_badge();
        assert_eq!((badge.width(), badge.height()), (16, 16));
        let alpha = |x: usize, y: usize| badge.rgba()[(y * 16 + x) * 4 + 3];
        assert_eq!(alpha(8, 8), 255);
        assert_eq!(alpha(0, 0), 0);
    }

    #[test]
    fn mention_tooltip_uses_russian_plural() {
        assert!(tooltip_with_mentions("FinchSpeak", 1, "ru").ends_with("1 упоминание"));
        assert!(tooltip_with_mentions("FinchSpeak", 3, "ru").ends_with("3 упоминания"));
        assert!(tooltip_with_mentions("FinchSpeak", 5, "ru").ends_with("5 упоминаний"));
        assert!(tooltip_with_mentions("FinchSpeak", 11, "ru").ends_with("11 упоминаний"));
        assert!(tooltip_with_mentions("FinchSpeak", 21, "ru").ends_with("21 упоминание"));
        assert_eq!(tooltip_with_mentions("FinchSpeak", 2, "en"), "FinchSpeak · 2 mentions");
        assert_eq!(tooltip_with_mentions("FinchSpeak", 0, "ru"), "FinchSpeak");
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
        for state in [TrayState::Idle, TrayState::Voice, TrayState::Speaking, TrayState::Muted, TrayState::Live] {
            assert!(state.tooltip("en").starts_with("FinchSpeak"));
            assert_eq!(state.dot().is_none(), state == TrayState::Idle);
        }
        assert_eq!(TrayState::Speaking.tooltip("ru"), "FinchSpeak: вы говорите");
        assert_eq!(TrayState::Speaking.tooltip("en"), "FinchSpeak: you are speaking");
        assert_eq!(TrayState::Speaking.dot(), Some([0x2E, 0xCC, 0x71]));
    }

    #[test]
    fn speaking_deserializes_from_the_tray_command_payload() {
        assert_eq!(serde_json::from_str::<TrayState>(r#""speaking""#).unwrap(), TrayState::Speaking);
    }

    #[test]
    fn tray_states_use_the_supplied_tray_picture() {
        let base = Image::from_bytes(TRAY_PICTURE).unwrap();
        assert_eq!((base.width(), base.height()), (64, 64));
        let idle = state_icon(&base, TrayState::Idle);
        assert_eq!(idle.rgba(), base.rgba());
        // The coloured dot is what tells the states apart.
        for state in [TrayState::Voice, TrayState::Speaking, TrayState::Muted, TrayState::Live] {
            let icon = state_icon(&base, state);
            assert_eq!((icon.width(), icon.height()), (64, 64));
            assert_ne!(icon.rgba(), idle.rgba(), "{state:?} should differ from idle");
        }
        assert_ne!(state_icon(&base, TrayState::Voice).rgba(), state_icon(&base, TrayState::Speaking).rgba());
        assert_ne!(state_icon(&base, TrayState::Voice).rgba(), state_icon(&base, TrayState::Muted).rgba());
    }
}
