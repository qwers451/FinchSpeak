//! Client application state shared across Tauri commands.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use finch_client_core::api::ApiClient;
use finch_client_core::session::ConnectParams;
pub use finch_client_core::api::UserDto;
use finch_audio::AudioMixer;
use finch_protocol::messages::ClientMessage;
use finch_client_core::screen::{ScreenAudioForwarder, ScreenSharer, ScreenViewer};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoChunkPayload {
    pub is_key: bool,
    pub data: Vec<u8>,
}

/// Audio devices the user picked; `None` means the system default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDeviceSelection {
    pub input: Option<String>,
    pub output: Option<String>,
}

impl AudioDeviceSelection {
    /// Build a selection; an empty name (an unset dropdown) means the default device.
    pub fn new(input: Option<String>, output: Option<String>) -> Self {
        let clean = |name: Option<String>| name.filter(|n| !n.trim().is_empty());
        Self { input: clean(input), output: clean(output) }
    }
}

/// What the user can tune about the microphone while talking. Read by the audio thread for every
/// frame, written by the UI and the push-to-talk hotkey, hence plain atomics.
pub struct VoiceSettings {
    input_gain: AtomicU32,
    vad_threshold_db: AtomicU32,
    push_to_talk: AtomicBool,
    key_down: AtomicBool,
    monitor: AtomicBool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            input_gain: AtomicU32::new(1.0f32.to_bits()),
            vad_threshold_db: AtomicU32::new(finch_audio::DEFAULT_VAD_THRESHOLD_DB.to_bits()),
            push_to_talk: AtomicBool::new(false),
            key_down: AtomicBool::new(false),
            monitor: AtomicBool::new(false),
        }
    }
}

/// The settings as the UI sees them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceSettingsDto {
    pub input_gain: f32,
    pub vad_threshold_db: f32,
    pub push_to_talk: bool,
    /// The push-to-talk key is held down right now.
    pub key_down: bool,
    pub monitor: bool,
}

impl VoiceSettings {
    pub fn input_gain(&self) -> f32 {
        f32::from_bits(self.input_gain.load(Ordering::Relaxed))
    }

    pub fn vad_threshold_db(&self) -> f32 {
        f32::from_bits(self.vad_threshold_db.load(Ordering::Relaxed))
    }

    pub fn monitor(&self) -> bool {
        self.monitor.load(Ordering::Relaxed)
    }

    /// May the microphone be transmitted right now? Always, unless push-to-talk wants a key held.
    pub fn transmit_open(&self) -> bool {
        !self.push_to_talk.load(Ordering::Relaxed) || self.key_down.load(Ordering::Relaxed)
    }

    pub fn set_key_down(&self, down: bool) {
        self.key_down.store(down, Ordering::Relaxed);
    }

    /// Apply the UI's values; out-of-range numbers are brought into range.
    pub fn apply(&self, new: &VoiceSettingsDto) {
        let gain = if new.input_gain.is_finite() { new.input_gain.clamp(0.0, finch_audio::MAX_INPUT_GAIN) } else { 1.0 };
        let threshold = if new.vad_threshold_db.is_finite() { new.vad_threshold_db.clamp(-80.0, 0.0) } else { finch_audio::DEFAULT_VAD_THRESHOLD_DB };
        self.input_gain.store(gain.to_bits(), Ordering::Relaxed);
        self.vad_threshold_db.store(threshold.to_bits(), Ordering::Relaxed);
        self.push_to_talk.store(new.push_to_talk, Ordering::Relaxed);
        self.monitor.store(new.monitor, Ordering::Relaxed);
        if !new.push_to_talk {
            self.key_down.store(false, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> VoiceSettingsDto {
        VoiceSettingsDto {
            input_gain: self.input_gain(),
            vad_threshold_db: self.vad_threshold_db(),
            push_to_talk: self.push_to_talk.load(Ordering::Relaxed),
            key_down: self.key_down.load(Ordering::Relaxed),
            monitor: self.monitor(),
        }
    }
}

pub struct AudioEngineHandle {
    pub is_running: Arc<AtomicBool>,
    pub is_muted: Arc<AtomicBool>,
    pub is_deafened: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct AppState {
    /// REST client for the current server (set on login/register).
    pub api: Arc<RwLock<Option<ApiClient>>>,
    pub server_url: Arc<RwLock<Option<String>>>,
    pub server_quic_addr: Arc<RwLock<Option<SocketAddr>>>,
    pub auth_token: Arc<RwLock<Option<String>>>,
    pub current_user: Arc<RwLock<Option<UserDto>>>,
    pub quic_connection: Arc<RwLock<Option<quinn::Connection>>>,
    pub control_tx: Arc<RwLock<Option<mpsc::Sender<ClientMessage>>>>,
    pub session_peer_id: Arc<AtomicU32>,
    pub current_voice_channel: Arc<RwLock<Option<u32>>>,
    pub is_muted: Arc<AtomicBool>,
    pub is_deafened: Arc<AtomicBool>,
    pub mixer: Arc<Mutex<AudioMixer>>,
    /// Receiving side of the screen share being watched (at most one at a time).
    pub screen_viewer: Arc<Mutex<ScreenViewer>>,
    /// Session peer id of the sharer being watched; 0 = not watching.
    pub viewing_peer: Arc<AtomicU32>,
    /// User whose share we watch; used to subscribe again after a reconnect.
    pub viewing_user: Arc<RwLock<Option<uuid::Uuid>>>,
    /// Our own running screen share, if any.
    pub screen_sharer: Arc<std::sync::Mutex<Option<ScreenSharer>>>,
    /// The sound of our shared screen, when we chose to share it.
    pub screen_audio: Arc<std::sync::Mutex<Option<ScreenAudioForwarder>>>,
    /// TOFU verified fingerprints: hostname -> SHA-256 hex string
    pub saved_fingerprints: Arc<DashMap<String, String>>,
    /// Parameters of the current QUIC session, reused to reconnect after a drop.
    pub last_connect: Arc<RwLock<Option<ConnectParams>>>,
    /// Bumped on every login and logout; a reconnect loop stops when it changes.
    pub session_generation: Arc<AtomicU64>,
    /// File with pinned server fingerprints (set at startup from the app data dir).
    pub known_servers_path: Arc<RwLock<Option<PathBuf>>>,
    /// Pending TOFU fingerprint to be accepted: (hostname, fingerprint)
    pub pending_fingerprint: Arc<RwLock<Option<(String, String)>>>,
    /// Screen video channel to WebCodecs in Webview
    pub video_channel: Arc<RwLock<Option<tauri::ipc::Channel<VideoChunkPayload>>>>,
    pub audio_engine: Arc<Mutex<crate::audio_engine::AudioEngine>>,
    /// Read by the audio worker thread when it opens devices, hence a std lock.
    pub audio_devices: Arc<std::sync::RwLock<AudioDeviceSelection>>,
    pub voice_settings: Arc<VoiceSettings>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Stop our screen share if one is running. Returns whether there was one.
    pub fn stop_screen_share(&self) -> bool {
        let audio = self.screen_audio.lock().ok().and_then(|mut slot| slot.take());
        if let Some(mut audio) = audio {
            audio.stop();
        }
        let sharer = self.screen_sharer.lock().ok().and_then(|mut slot| slot.take());
        match sharer {
            Some(mut sharer) => {
                sharer.stop();
                true
            }
            None => false,
        }
    }

    /// Forget the screen share we were watching.
    pub async fn stop_watching(&self) {
        self.viewing_peer.store(0, std::sync::atomic::Ordering::SeqCst);
        *self.viewing_user.write().await = None;
        *self.screen_viewer.lock().await = ScreenViewer::new();
    }

    pub fn new() -> Self {
        Self {
            api: Arc::new(RwLock::new(None)),
            server_url: Arc::new(RwLock::new(None)),
            server_quic_addr: Arc::new(RwLock::new(None)),
            auth_token: Arc::new(RwLock::new(None)),
            current_user: Arc::new(RwLock::new(None)),
            quic_connection: Arc::new(RwLock::new(None)),
            control_tx: Arc::new(RwLock::new(None)),
            session_peer_id: Arc::new(AtomicU32::new(0)),
            current_voice_channel: Arc::new(RwLock::new(None)),
            is_muted: Arc::new(AtomicBool::new(false)),
            is_deafened: Arc::new(AtomicBool::new(false)),
            mixer: Arc::new(Mutex::new(AudioMixer::new())),
            screen_viewer: Arc::new(Mutex::new(ScreenViewer::new())),
            viewing_peer: Arc::new(AtomicU32::new(0)),
            viewing_user: Arc::new(RwLock::new(None)),
            screen_sharer: Arc::new(std::sync::Mutex::new(None)),
            screen_audio: Arc::new(std::sync::Mutex::new(None)),
            saved_fingerprints: Arc::new(DashMap::new()),
            known_servers_path: Arc::new(RwLock::new(None)),
            last_connect: Arc::new(RwLock::new(None)),
            session_generation: Arc::new(AtomicU64::new(0)),
            pending_fingerprint: Arc::new(RwLock::new(None)),
            video_channel: Arc::new(RwLock::new(None)),
            audio_engine: Arc::new(Mutex::new(crate::audio_engine::AudioEngine::new())),
            audio_devices: Arc::new(std::sync::RwLock::new(AudioDeviceSelection::default())),
            voice_settings: Arc::new(VoiceSettings::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_device_names_mean_default() {
        let selection = AudioDeviceSelection::new(Some("".into()), Some("  ".into()));
        assert_eq!(selection, AudioDeviceSelection::default());
        let picked = AudioDeviceSelection::new(Some("Headset".into()), None);
        assert_eq!(picked.input.as_deref(), Some("Headset"));
        assert_eq!(picked.output, None);
    }
}

#[cfg(test)]
mod voice_settings_tests {
    use super::*;

    fn dto(gain: f32, threshold: f32, ptt: bool) -> VoiceSettingsDto {
        VoiceSettingsDto { input_gain: gain, vad_threshold_db: threshold, push_to_talk: ptt, key_down: false, monitor: false }
    }

    #[test]
    fn defaults_transmit_normally() {
        let settings = VoiceSettings::default();
        assert_eq!(settings.input_gain(), 1.0);
        assert!(settings.transmit_open());
        assert!(!settings.monitor());
    }

    #[test]
    fn push_to_talk_transmits_only_while_the_key_is_down() {
        let settings = VoiceSettings::default();
        settings.apply(&dto(1.0, -45.0, true));
        assert!(!settings.transmit_open());
        settings.set_key_down(true);
        assert!(settings.transmit_open());
        settings.set_key_down(false);
        assert!(!settings.transmit_open());
        // Switching push-to-talk off must not leave the microphone stuck closed or open by a stale key.
        settings.set_key_down(true);
        settings.apply(&dto(1.0, -45.0, false));
        assert!(settings.transmit_open());
        settings.apply(&dto(1.0, -45.0, true));
        assert!(!settings.transmit_open(), "the old key press is forgotten");
    }

    #[test]
    fn values_from_the_ui_are_brought_into_range() {
        let settings = VoiceSettings::default();
        settings.apply(&dto(100.0, 5.0, false));
        assert_eq!(settings.input_gain(), finch_audio::MAX_INPUT_GAIN);
        assert_eq!(settings.vad_threshold_db(), 0.0);
        settings.apply(&dto(-3.0, -500.0, false));
        assert_eq!(settings.input_gain(), 0.0);
        assert_eq!(settings.vad_threshold_db(), -80.0);
        settings.apply(&dto(f32::NAN, f32::NAN, false));
        assert_eq!(settings.input_gain(), 1.0);
        assert_eq!(settings.vad_threshold_db(), finch_audio::DEFAULT_VAD_THRESHOLD_DB);
    }

    #[test]
    fn the_snapshot_reflects_what_was_applied() {
        let settings = VoiceSettings::default();
        let mut wanted = dto(2.0, -30.0, true);
        wanted.monitor = true;
        settings.apply(&wanted);
        assert_eq!(settings.snapshot(), wanted);
    }
}
