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
use finch_screen::FrameAssembler;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoChunkPayload {
    pub is_key: bool,
    pub data: Vec<u8>,
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
    pub assembler: Arc<Mutex<FrameAssembler>>,
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
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
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
            assembler: Arc::new(Mutex::new(FrameAssembler::new())),
            saved_fingerprints: Arc::new(DashMap::new()),
            known_servers_path: Arc::new(RwLock::new(None)),
            last_connect: Arc::new(RwLock::new(None)),
            session_generation: Arc::new(AtomicU64::new(0)),
            pending_fingerprint: Arc::new(RwLock::new(None)),
            video_channel: Arc::new(RwLock::new(None)),
            audio_engine: Arc::new(Mutex::new(crate::audio_engine::AudioEngine::new())),
        }
    }
}
