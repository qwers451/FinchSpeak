//! System-wide keys: push to talk, mute the microphone, and turn the sound off.
//! They work while FinchSpeak is in the background.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Held: the microphone is open while the key is down.
    PushToTalk,
    /// Pressed: mute or unmute the microphone.
    ToggleMute,
    /// Pressed: turn all sound off or back on.
    ToggleDeafen,
}

impl HotkeyAction {
    const ALL: [HotkeyAction; 3] = [HotkeyAction::PushToTalk, HotkeyAction::ToggleMute, HotkeyAction::ToggleDeafen];

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "push_to_talk" => Some(Self::PushToTalk),
            "toggle_mute" => Some(Self::ToggleMute),
            "toggle_deafen" => Some(Self::ToggleDeafen),
            _ => None,
        }
    }

    /// The name the UI uses, also sent with the `hotkey` event.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PushToTalk => "push_to_talk",
            Self::ToggleMute => "toggle_mute",
            Self::ToggleDeafen => "toggle_deafen",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

type Keys = [Option<Shortcut>; 3];

/// Which key each action uses.
#[derive(Default)]
pub struct Hotkeys {
    keys: Mutex<Keys>,
    /// The push-to-talk key state last reported to the UI; auto-repeat does not change it.
    push_to_talk_down: AtomicBool,
}

impl Hotkeys {
    fn action_for(&self, shortcut: &Shortcut) -> Option<HotkeyAction> {
        let keys = self.keys.lock().ok()?;
        find_action(&keys, shortcut)
    }
}

fn find_action(keys: &Keys, shortcut: &Shortcut) -> Option<HotkeyAction> {
    HotkeyAction::ALL.into_iter().find(|action| keys[action.index()].as_ref() == Some(shortcut))
}

/// Another action that already uses this key.
fn conflict(keys: &Keys, action: HotkeyAction, shortcut: &Shortcut) -> Option<HotkeyAction> {
    find_action(keys, shortcut).filter(|other| *other != action)
}

/// Whether `pressed` changes the key state kept in `down`. Holding a key makes the platform send
/// the press again and again (auto-repeat), so only the first press and the release count.
fn key_changed(down: &AtomicBool, pressed: bool) -> bool {
    down.swap(pressed, Ordering::SeqCst) != pressed
}

/// Called by the global-shortcut plugin for every registered key.
pub fn handle(app: &AppHandle, shortcut: &Shortcut, key_state: ShortcutState) {
    let state = app.state::<AppState>();
    let Some(action) = state.hotkeys.action_for(shortcut) else { return };
    let pressed = key_state == ShortcutState::Pressed;
    match action {
        HotkeyAction::PushToTalk => {
            state.voice_settings.set_key_down(pressed);
            if key_changed(&state.hotkeys.push_to_talk_down, pressed) {
                let _ = app.emit("push-to-talk", serde_json::json!({ "pressed": pressed }));
            }
        }
        // The UI owns mute and deafen (it keeps "muted before deafen" and the buttons in sync).
        _ if pressed => {
            let _ = app.emit("hotkey", action.as_str());
        }
        _ => {}
    }
}

/// Use `key` (e.g. "F9", "Control+Shift+M") for `action`; `None` removes it.
pub fn set(app: &AppHandle, state: &AppState, action: HotkeyAction, key: Option<&str>) -> Result<(), String> {
    let text = key.map(str::trim).filter(|key| !key.is_empty());
    let wanted = match text {
        Some(text) => Some(text.parse::<Shortcut>().map_err(|e| format!("\"{text}\" cannot be used as a key: {e}"))?),
        None => None,
    };
    let old = {
        let keys = state.hotkeys.keys.lock().map_err(|_| "Keys are unavailable".to_string())?;
        if let (Some(text), Some(shortcut)) = (text, wanted.as_ref()) {
            if conflict(&keys, action, shortcut).is_some() {
                return Err(format!("\"{text}\" is already used by another action"));
            }
        }
        keys[action.index()]
    };
    if old == wanted {
        return Ok(());
    }
    // The plugin may run on the main thread, so the lock is not held while it works.
    let manager = app.global_shortcut();
    if let Some(shortcut) = wanted {
        manager.register(shortcut).map_err(|e| format!("Could not use \"{}\" as a key: {e}", text.unwrap_or_default()))?;
    }
    if let Some(shortcut) = old {
        if let Err(e) = manager.unregister(shortcut) {
            tracing::warn!("Could not release the old key: {e}");
        }
    }
    if let Ok(mut keys) = state.hotkeys.keys.lock() {
        keys[action.index()] = wanted;
    }
    if action == HotkeyAction::PushToTalk {
        // A key that is no longer ours cannot be released by us.
        state.voice_settings.set_key_down(false);
        // A release we never saw must not make the next press look like a repeat.
        state.hotkeys.push_to_talk_down.store(false, Ordering::SeqCst);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Shortcut {
        text.parse().unwrap()
    }

    #[test]
    fn actions_round_trip_through_their_names() {
        for action in HotkeyAction::ALL {
            assert_eq!(HotkeyAction::parse(action.as_str()), Some(action));
        }
        assert_eq!(HotkeyAction::parse("launch_rockets"), None);
    }

    #[test]
    fn a_key_belongs_to_one_action() {
        let mut keys: Keys = [None; 3];
        keys[HotkeyAction::ToggleMute.index()] = Some(key("F9"));
        assert_eq!(find_action(&keys, &key("F9")), Some(HotkeyAction::ToggleMute));
        assert_eq!(find_action(&keys, &key("F10")), None);
        assert_eq!(conflict(&keys, HotkeyAction::ToggleDeafen, &key("F9")), Some(HotkeyAction::ToggleMute));
        assert_eq!(conflict(&keys, HotkeyAction::ToggleMute, &key("F9")), None, "keeping its own key is fine");
    }

    #[test]
    fn repeated_presses_are_one_change() {
        let down = AtomicBool::new(false);
        assert!(key_changed(&down, true), "the first press is a change");
        assert!(!key_changed(&down, true), "auto-repeat is not");
        assert!(!key_changed(&down, true), "auto-repeat is not");
        assert!(key_changed(&down, false), "the release is a change");
        assert!(!key_changed(&down, false), "a repeated release is not");
    }

    #[test]
    fn modifiers_make_a_different_key() {
        let mut keys: Keys = [None; 3];
        keys[HotkeyAction::PushToTalk.index()] = Some(key("Control+M"));
        assert_eq!(find_action(&keys, &key("M")), None);
        assert_eq!(find_action(&keys, &key("Control+M")), Some(HotkeyAction::PushToTalk));
    }
}
