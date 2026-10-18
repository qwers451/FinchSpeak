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
    // Low bits: PTT, key, muted, mic-test; high bits: monotonic generation.
    // One CAS publishes controls and epoch together, including rapid close/open.
    privacy: AtomicU64,
    monitor: AtomicBool,
    voice_enhancement: AtomicBool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            input_gain: AtomicU32::new(1.0f32.to_bits()),
            vad_threshold_db: AtomicU32::new(finch_audio::DEFAULT_VAD_THRESHOLD_DB.to_bits()),
            privacy: AtomicU64::new(16),
            monitor: AtomicBool::new(false),
            voice_enhancement: AtomicBool::new(true),
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
    #[serde(default = "default_voice_enhancement")]
    pub voice_enhancement: bool,
}

fn default_voice_enhancement() -> bool { true }

impl VoiceSettings {
    pub fn voice_enhancement(&self) -> bool {
        self.voice_enhancement.load(Ordering::Relaxed)
    }

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
        let bits = self.privacy.load(Ordering::SeqCst);
        bits & 1 == 0 || bits & 2 != 0
    }

    pub fn set_key_down(&self, down: bool) {
        self.update_privacy(|bits| if down && bits & 1 != 0 { bits | 2 } else { bits & !2 });
    }

    fn update_privacy(&self, change: impl Fn(u64) -> u64) {
        let _ = self.privacy.try_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
            let flags = change(old) & 15;
            if flags == old & 15 { None } else { Some(((old & !15) + 16) | flags) }
        });
    }

    pub(crate) fn privacy_snapshot(&self) -> (u64, bool) {
        let bits = self.privacy.load(Ordering::SeqCst);
        (bits, bits & 12 == 0 && (bits & 1 == 0 || bits & 2 != 0))
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        self.update_privacy(|bits| if muted { bits | 4 } else { bits & !4 });
    }

    pub(crate) fn set_mic_test(&self, active: bool) {
        self.update_privacy(|bits| if active { bits | 8 } else { bits & !8 });
    }

    /// Apply the UI's values; out-of-range numbers are brought into range.
    pub fn apply(&self, new: &VoiceSettingsDto) {
        let gain = if new.input_gain.is_finite() { new.input_gain.clamp(0.0, finch_audio::MAX_INPUT_GAIN) } else { 1.0 };
        let threshold = if new.vad_threshold_db.is_finite() { new.vad_threshold_db.clamp(-80.0, 0.0) } else { finch_audio::DEFAULT_VAD_THRESHOLD_DB };
        self.input_gain.store(gain.to_bits(), Ordering::Relaxed);
        self.vad_threshold_db.store(threshold.to_bits(), Ordering::Relaxed);
        self.update_privacy(|bits| if new.push_to_talk { bits | 1 } else { bits & !3 });
        self.monitor.store(new.monitor, Ordering::Relaxed);
        self.voice_enhancement.store(new.voice_enhancement, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> VoiceSettingsDto {
        let privacy = self.privacy.load(Ordering::SeqCst);
        VoiceSettingsDto {
            input_gain: self.input_gain(),
            vad_threshold_db: self.vad_threshold_db(),
            push_to_talk: privacy & 1 != 0,
            key_down: privacy & 2 != 0,
            monitor: self.monitor(),
            voice_enhancement: self.voice_enhancement(),
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
    /// Seconds of microphone test requested by the UI (0 = none); the audio worker takes it.
    /// An update found by `check_update`, waiting for the person to accept it.
    pub pending_update: Arc<tokio::sync::Mutex<Option<(crate::updater::UpdateInfo, crate::updater::PlatformEntry)>>>,
    /// Closing the window hides it to the tray instead of quitting.
    pub close_to_tray: Arc<std::sync::atomic::AtomicBool>,
    /// The user chose Quit: really close.
    pub quitting: Arc<std::sync::atomic::AtomicBool>,
    pub mic_test_request: Arc<std::sync::atomic::AtomicU32>,
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
        let peer_id = self.viewing_peer.swap(0, std::sync::atomic::Ordering::SeqCst);
        if peer_id != 0 {
            self.mixer.lock().await.retire_stream(finch_audio::screen_audio_key(peer_id));
        }
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
            pending_update: Arc::new(tokio::sync::Mutex::new(None)),
            close_to_tray: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            quitting: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mic_test_request: Arc::new(std::sync::atomic::AtomicU32::new(0)),
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

    #[tokio::test]
    async fn stopping_a_view_retires_screen_audio_without_removing_voice() {
        use finch_audio::{screen_audio_key, OpusVoiceEncoder, FRAME_SIZE_48K};
        use finch_protocol::media::MediaHeader;
        let state = AppState::new();
        let mut encoder = OpusVoiceEncoder::new(64).unwrap();
        let payload = encoder.encode(&[0.1; FRAME_SIZE_48K]).unwrap();
        {
            let mut mixer = state.mixer.lock().await;
            mixer.set_speaker_volume(screen_audio_key(7), 0.0);
            for seq in 0..2 {
                let header = MediaHeader::new_audio(1, 7, seq, seq as u32 * 20, false, false);
                mixer.on_audio_packet(&header, payload.clone(), std::time::Instant::now());
                mixer.on_screen_audio_packet(&header, payload.clone(), std::time::Instant::now());
            }
            assert!(mixer.loss_rate(screen_audio_key(7)).is_some());
        }
        state.viewing_peer.store(7, Ordering::SeqCst);
        state.stop_watching().await;
        let mut mixer = state.mixer.lock().await;
        assert!(mixer.loss_rate(screen_audio_key(7)).is_none());
        assert!(mixer.loss_rate(7).is_some(), "participant voice remains available");
        mixer.set_speaker_volume(7, 0.0);
        for seq in 0..2 {
            let header = MediaHeader::new_audio(1, 7, seq, seq as u32 * 20, false, false);
            mixer.on_screen_audio_packet(&header, payload.clone(), std::time::Instant::now());
        }
        let mut out = [0.0; FRAME_SIZE_48K];
        mixer.mix_frame(&mut out, std::time::Instant::now());
        assert!(out.iter().all(|sample| *sample == 0.0), "watching again preserves the saved screen mute");
    }

    fn dto(gain: f32, threshold: f32, ptt: bool) -> VoiceSettingsDto {
        VoiceSettingsDto { input_gain: gain, vad_threshold_db: threshold, push_to_talk: ptt, key_down: false, monitor: false, voice_enhancement: true }
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
    #[test]
    fn enhancement_defaults_migrates_and_round_trips() {
        let settings = VoiceSettings::default();
        assert!(settings.voice_enhancement());
        let old = r#"{"input_gain":1.5,"vad_threshold_db":-50,"push_to_talk":false,"key_down":false,"monitor":false}"#;
        let mut migrated: VoiceSettingsDto = serde_json::from_str(old).unwrap();
        assert!(migrated.voice_enhancement);
        assert_eq!(migrated.input_gain, 1.5);
        assert_eq!(migrated.vad_threshold_db, -50.0);
        for enabled in [false, true] {
            migrated.voice_enhancement = enabled;
            settings.apply(&migrated);
            assert_eq!(settings.voice_enhancement(), enabled);
            assert_eq!(settings.snapshot(), migrated);
            assert_eq!(serde_json::from_str::<VoiceSettingsDto>(&serde_json::to_string(&migrated).unwrap()).unwrap(), migrated);
        }
    }}
