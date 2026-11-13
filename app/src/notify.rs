#![cfg_attr(not(windows), allow(dead_code))]
//! Windows notifications with the sender's picture, two buttons and a click that brings the window
//! up. The Tauri plugin cannot report clicks or show buttons, so the toast is built here. Other
//! systems refuse this command and the frontend keeps using the plugin.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Longest title or text a notification shows.
const MAX_CHARS: usize = 200;
/// Largest picture written for a notification (avatars are 128 px, a few kilobytes).
const MAX_PICTURE_BYTES: usize = 512 * 1024;
/// The app identifier from tauri.conf.json; the installer gives the Start menu shortcut the same id.
pub const APP_ID: &str = "com.finchspeak.app";

/// Where a click leads: a text channel, a voice room, or nowhere in particular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoticeTarget {
    Channel { id: String },
    VoiceRoom { id: String },
    None,
}

/// A button of the notification; `id` comes back on a click.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoticeAction {
    pub id: String,
    pub label: String,
}

/// A notification as the interface describes it. The decision to show it was taken there.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// The sender: their picture is shown round. Only a parsed UUID ever names a file.
    #[serde(default)]
    pub avatar_user_id: Option<Uuid>,
    pub target: NoticeTarget,
    #[serde(default)]
    pub actions: Vec<NoticeAction>,
}

/// What the window hears about a click: where to go, and the pressed button if one was pressed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Activation {
    pub target: NoticeTarget,
    pub action: Option<String>,
}

/// Everything a toast needs, built without touching Windows so it can be tested.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastSpec {
    pub app_id: String,
    pub title: String,
    pub text: String,
    pub picture: Option<PathBuf>,
    /// Button label and the action id sent back on a click.
    pub buttons: Vec<(String, String)>,
}

/// Cut to the notification limit, by characters so no letter is split.
pub fn clip(text: &str) -> String {
    text.chars().take(MAX_CHARS).collect()
}

/// Where the picture of `user_id` is kept. A UUID cannot carry path separators.
pub fn avatar_path(cache_dir: &Path, user_id: Uuid) -> PathBuf {
    cache_dir.join("notify-avatars").join(format!("{user_id}.png"))
}

/// The toast for `notice`. Markup characters stay as they are: the toast XML is built by the DOM,
/// which escapes them.
pub fn toast_spec(notice: &Notice, picture: Option<PathBuf>, app_id: &str) -> ToastSpec {
    ToastSpec {
        app_id: app_id.to_string(),
        title: clip(&notice.title),
        text: clip(&notice.body),
        picture,
        buttons: notice
            .actions
            .iter()
            .filter(|action| !action.id.is_empty())
            .take(2)
            .map(|action| (clip(&action.label), action.id.clone()))
            .collect(),
    }
}

/// The click as the window hears it. A button id the notification did not offer is dropped.
pub fn activation(target: &NoticeTarget, offered: &[String], pressed: Option<String>) -> Activation {
    let action = pressed.filter(|id| offered.iter().any(|known| known == id));
    Activation { target: target.clone(), action }
}

/// Write the picture into the cache folder and return its path; oversized or empty pictures are ignored.
fn store_picture(cache_dir: &Path, user_id: Uuid, bytes: &[u8]) -> Result<Option<PathBuf>, String> {
    if bytes.is_empty() || bytes.len() > MAX_PICTURE_BYTES {
        return Ok(None);
    }
    let path = avatar_path(cache_dir, user_id);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(Some(path))
}

/// Show `notice`; a click emits `notice-activated` and brings the window up.
#[cfg(windows)]
pub fn send(app: &tauri::AppHandle, notice: Notice, avatar_png: Option<Vec<u8>>, cache_dir: PathBuf) -> Result<(), String> {
    use tauri::Emitter;

    let picture = match (notice.avatar_user_id, avatar_png) {
        (Some(user_id), Some(bytes)) => store_picture(&cache_dir, user_id, &bytes)?,
        _ => None,
    };
    let spec = toast_spec(&notice, picture, &app_id());
    let target = notice.target.clone();
    let offered: Vec<String> = notice.actions.iter().map(|action| action.id.clone()).collect();
    let app = app.clone();

    let mut toast = tauri_winrt_notification::Toast::new(&spec.app_id).title(&spec.title);
    if !spec.text.is_empty() {
        toast = toast.text1(&spec.text);
    }
    if let Some(picture) = &spec.picture {
        toast = toast.icon(picture, tauri_winrt_notification::IconCrop::Circular, "");
    }
    for (label, action) in &spec.buttons {
        toast = toast.add_button(label, action);
    }
    toast
        .on_activated(move |pressed| {
            let heard = activation(&target, &offered, pressed);
            let _ = app.emit("notice-activated", heard);
            crate::tray::show_window(&app);
            Ok(())
        })
        .show()
        .map_err(|e| e.to_string())
}

/// Other systems: the caller falls back to the notification plugin.
#[cfg(not(windows))]
pub fn send(_app: &tauri::AppHandle, _notice: Notice, _avatar_png: Option<Vec<u8>>, _cache_dir: PathBuf) -> Result<(), String> {
    Err("Notifications with pictures and buttons are only available on Windows".into())
}

/// The application id the toast is shown under. Unpackaged builds (cargo target folders) have no
/// registered id, so they use PowerShell's, as the notification plugin does.
#[cfg(windows)]
fn app_id() -> String {
    let unpackaged = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .is_some_and(|dir| dir.ends_with(Path::new("target").join("debug")) || dir.ends_with(Path::new("target").join("release")));
    if unpackaged {
        tauri_winrt_notification::Toast::POWERSHELL_APP_ID.to_string()
    } else {
        APP_ID.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(body: &str) -> Notice {
        Notice {
            title: "Ada zовёт в Standup".into(),
            body: body.into(),
            avatar_user_id: Some(Uuid::nil()),
            target: NoticeTarget::VoiceRoom { id: "room".into() },
            actions: vec![
                NoticeAction { id: "fly".into(), label: "Лететь".into() },
                NoticeAction { id: "stay".into(), label: "Остаться".into() },
            ],
        }
    }

    #[test]
    fn notify_toast_carries_title_text_picture_and_buttons() {
        let picture = PathBuf::from("C:/cache/notify-avatars/x.png");
        let spec = toast_spec(&sample("hello"), Some(picture.clone()), APP_ID);
        assert_eq!(spec.app_id, APP_ID);
        assert_eq!(spec.title, "Ada zовёт в Standup");
        assert_eq!(spec.text, "hello");
        assert_eq!(spec.picture, Some(picture));
        assert_eq!(
            spec.buttons,
            vec![("Лететь".to_string(), "fly".to_string()), ("Остаться".to_string(), "stay".to_string())]
        );
    }

    #[test]
    fn notify_keeps_markup_characters_as_plain_text() {
        let spec = toast_spec(&sample("a < b & \"c\" 'd'"), None, APP_ID);
        assert_eq!(spec.text, "a < b & \"c\" 'd'");
        assert!(spec.picture.is_none());
    }

    #[test]
    fn notify_clips_long_text_by_characters() {
        let long = "я".repeat(500);
        assert_eq!(clip(&long).chars().count(), MAX_CHARS);
    }

    #[test]
    fn notify_avatar_path_stays_in_cache_folder() {
        let cache = Path::new("C:/cache");
        let path = avatar_path(cache, Uuid::nil());
        assert!(path.starts_with(cache));
        assert_eq!(path.parent(), Some(cache.join("notify-avatars").as_path()));
    }

    #[test]
    fn notify_rejects_unsafe_avatar_id_before_any_path_is_built() {
        let unsafe_id = r#"{"title":"t","body":"b","avatar_user_id":"../../evil","target":{"kind":"none"}}"#;
        assert!(serde_json::from_str::<Notice>(unsafe_id).is_err());
        let fine = r#"{"title":"t","body":"b","avatar_user_id":"00000000-0000-0000-0000-000000000000","target":{"kind":"none"}}"#;
        assert!(serde_json::from_str::<Notice>(fine).is_ok());
    }

    #[test]
    fn notify_click_reports_only_offered_buttons() {
        let target = NoticeTarget::Channel { id: "c".into() };
        let offered = vec!["fly".to_string(), "stay".to_string()];
        assert_eq!(activation(&target, &offered, Some("fly".into())).action.as_deref(), Some("fly"));
        assert_eq!(activation(&target, &offered, Some("delete".into())).action, None);
        assert_eq!(activation(&target, &offered, None).action, None);
        assert_eq!(activation(&target, &offered, None).target, target);
    }

    #[test]
    fn notify_empty_or_oversized_picture_is_not_written() {
        let dir = std::env::temp_dir().join("finch-notify-test-none");
        assert_eq!(store_picture(&dir, Uuid::nil(), &[]).unwrap(), None);
        assert_eq!(store_picture(&dir, Uuid::nil(), &vec![0u8; MAX_PICTURE_BYTES + 1]).unwrap(), None);
    }
}
