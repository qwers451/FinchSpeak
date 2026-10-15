//! Client application state shared across Tauri commands.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::Arc;

use dashmap::DashMap;
use finch_client_core::api::ApiClient;
use finch_client_core::session::ConnectParams;
pub use finch_client_core::api::UserDto;
use finch_audio::AudioMixer;
use finch_protocol::messages::ClientMessage;
use finch_client_core::screen::{ScreenSharer, ScreenViewer};
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
    /// Our own running screen share, if any.
    pub screen_sharer: Arc<std::sync::Mutex<Option<ScreenSharer>>>,
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
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Stop our screen share if one is running. Returns whether there was one.
    pub fn stop_screen_share(&self) -> bool {
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
            screen_sharer: Arc::new(std::sync::Mutex::new(None)),
            saved_fingerprints: Arc::new(DashMap::new()),
            known_servers_path: Arc::new(RwLock::new(None)),
            last_connect: Arc::new(RwLock::new(None)),
            session_generation: Arc::new(AtomicU64::new(0)),
            pending_fingerprint: Arc::new(RwLock::new(None)),
            video_channel: Arc::new(RwLock::new(None)),
            audio_engine: Arc::new(Mutex::new(crate::audio_engine::AudioEngine::new())),
            audio_devices: Arc::new(std::sync::RwLock::new(AudioDeviceSelection::default())),
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
