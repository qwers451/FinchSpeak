//! QUIC session wiring: forwards `finch-client-core` session events to Tauri events,
//! the audio mixer and the screen-share frame assembler, and reconnects after a drop.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use finch_client_core::error::ClientError;
use finch_client_core::session::{
    connect, reconnect_with_backoff, Backoff, ConnectParams, Session, SessionEvent,
};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use quinn::Connection;
use tauri::Emitter;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::state::{AppState, VideoChunkPayload};

/// Establish QUIC connection and spawn the session loop.
pub async fn start_quic_session(
    server_addr: SocketAddr,
    hostname: &str,
    token: String,
    state: AppState,
    app_handle: tauri::AppHandle,
) -> anyhow::Result<Connection> {
    let params = ConnectParams {
        quic_addr: server_addr,
        hostname: hostname.to_string(),
        token,
        require_known_fingerprint: true,
    };
    state.session_generation.fetch_add(1, Ordering::SeqCst);
    let connected = connect(params.clone(), state.saved_fingerprints.clone()).await;
    let (session, events) = match connected {
        Ok(ok) => ok,
        Err(ClientError::UntrustedServer { hostname, fingerprint }) => {
            *state.pending_fingerprint.write().await = Some((hostname.clone(), fingerprint.clone()));
            let _ = app_handle.emit(
                "tofu-required",
                serde_json::json!({ "hostname": hostname, "fingerprint": fingerprint }),
            );
            anyhow::bail!("server certificate for {hostname} is not trusted yet — confirm its fingerprint and sign in again");
        }
        Err(e) => return Err(e.into()),
    };

    *state.last_connect.write().await = Some(params);
    let connection = session.connection.clone();
    activate_session(&session, &state, &app_handle).await;
    tokio::spawn(run_session(events, state, app_handle));
    Ok(connection)
}

/// Make `session` the active one in the app state and tell the UI.
async fn activate_session(session: &Session, state: &AppState, app_handle: &tauri::AppHandle) {
    state
        .session_peer_id
        .store(session.session_peer_id, Ordering::SeqCst);
    let _ = app_handle.emit(
        "authenticated",
        serde_json::json!({
            "session_peer_id": session.session_peer_id,
            "user_id": session.user_id,
            "channels": session.channels,
            "initial_state": session.initial_state
        }),
    );
    *state.control_tx.write().await = Some(session.control_tx.clone());
    *state.quic_connection.write().await = Some(session.connection.clone());
}

/// Forward session events; after a drop, reconnect and continue with the new session.
async fn run_session(
    mut events: mpsc::Receiver<SessionEvent>,
    state: AppState,
    app_handle: tauri::AppHandle,
) {
    loop {
        let Some(reason) = forward_events(&mut events, &state, &app_handle).await else {
            return;
        };
        let _ = app_handle.emit("connection-lost", reason);
        match reconnect(&state, &app_handle).await {
            Some(new_events) => events = new_events,
            None => return,
        }
    }
}

/// Forward events until the connection is lost (returns the reason) or the stream ends.
async fn forward_events(
    events: &mut mpsc::Receiver<SessionEvent>,
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Option<String> {
    while let Some(event) = events.recv().await {
        match event {
            SessionEvent::Control(msg) => emit_control(msg, app_handle),
            SessionEvent::Datagram(bytes) => route_datagram(bytes, state, app_handle).await,
            SessionEvent::UniStream { header, payload } => {
                if header.media_type == MediaType::VideoKey {
                    let mut asm = state.assembler.lock().await;
                    let frame = asm.on_keyframe_received(header.frame_id, header.timestamp, payload);
                    if let Some(ch) = &*state.video_channel.read().await {
                        let _ = ch.send(VideoChunkPayload {
                            is_key: true,
                            data: frame.data.to_vec(),
                        });
                    }
                }
            }
            SessionEvent::ConnectionLost(reason) => return Some(reason),
        }
    }
    None
}

/// Reconnect with backoff and rejoin the voice channel the user was in. Returns `None`
/// when the user logged out or in again meanwhile, or when retrying cannot help.
async fn reconnect(
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Option<mpsc::Receiver<SessionEvent>> {
    let generation = state.session_generation.load(Ordering::SeqCst);
    let params = state.last_connect.read().await.clone()?;
    let _ = app_handle.emit("reconnecting", ());

    let superseded = async {
        while state.session_generation.load(Ordering::SeqCst) == generation {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };
    let result = tokio::select! {
        r = reconnect_with_backoff(params, state.saved_fingerprints.clone(), Backoff::default()) => r,
        _ = superseded => {
            info!("Reconnect cancelled: session was replaced or logged out");
            return None;
        }
    };

    match result {
        Ok((session, events)) => {
            let voice_channel = (*state.current_voice_channel.read().await).and_then(|short_id| {
                session
                    .channels
                    .iter()
                    .find(|c| c.short_id == short_id)
                    .map(|c| c.id)
            });
            activate_session(&session, state, app_handle).await;
            if let Some(channel_id) = voice_channel {
                let _ = session
                    .control_tx
                    .send(ClientMessage::JoinVoice { channel_id })
                    .await;
            }
            let _ = app_handle.emit("reconnected", ());
            Some(events)
        }
        Err(e) => {
            warn!("Giving up reconnecting: {e}");
            let _ = app_handle.emit("reconnect-failed", e.to_string());
            None
        }
    }
}

fn emit_control(msg: ServerMessage, app_handle: &tauri::AppHandle) {
    match msg {
        ServerMessage::NewMessage {
            message_id,
            channel_id,
            author_id,
            author_name,
            content,
            attachments,
            timestamp,
        } => {
            let _ = app_handle.emit(
                "new-message",
                serde_json::json!({
                    "message_id": message_id,
                    "channel_id": channel_id,
                    "author_id": author_id,
                    "author_name": author_name,
                    "content": content,
                    "attachments": attachments,
                    "timestamp": timestamp,
                }),
            );
        }
        ServerMessage::MessageHistory {
            channel_id,
            messages,
            has_more,
        } => {
            let _ = app_handle.emit(
                "message-history",
                serde_json::json!({
                    "channel_id": channel_id,
                    "messages": messages,
                    "has_more": has_more,
                }),
            );
        }
        ServerMessage::UserJoinedVoice {
            user_id,
            peer_id,
            username,
            channel_id,
        } => {
            let _ = app_handle.emit(
                "user-joined-voice",
                serde_json::json!({
                    "user_id": user_id,
                    "peer_id": peer_id,
                    "username": username,
                    "channel_id": channel_id,
                }),
            );
        }
        ServerMessage::UserLeftVoice {
            user_id,
            peer_id,
            channel_id,
        } => {
            let _ = app_handle.emit(
                "user-left-voice",
                serde_json::json!({
                    "user_id": user_id,
                    "peer_id": peer_id,
                    "channel_id": channel_id,
                }),
            );
        }
        ServerMessage::UserVoiceStateChanged {
            peer_id,
            muted,
            deafened,
        } => {
            let _ = app_handle.emit(
                "user-voice-state-changed",
                serde_json::json!({
                    "peer_id": peer_id,
                    "muted": muted,
                    "deafened": deafened,
                }),
            );
        }
        ServerMessage::ScreenShareStarted {
            user_id,
            peer_id,
            codec,
            width,
            height,
            fps,
        } => {
            let _ = app_handle.emit(
                "screen-share-started",
                serde_json::json!({
                    "user_id": user_id,
                    "peer_id": peer_id,
                    "codec": codec,
                    "width": width,
                    "height": height,
                    "fps": fps,
                }),
            );
        }
        ServerMessage::ScreenShareStopped { peer_id } => {
            let _ = app_handle.emit(
                "screen-share-stopped",
                serde_json::json!({ "peer_id": peer_id }),
            );
        }
        ServerMessage::PresenceUpdate { user_id, status } => {
            let _ = app_handle.emit(
                "presence-update",
                serde_json::json!({
                    "user_id": user_id,
                    "status": status,
                }),
            );
        }
        ServerMessage::ChannelCreated { channel } => {
            let _ = app_handle.emit("channel-created", serde_json::json!({ "channel": channel }));
        }
        ServerMessage::KeyframeRequested => {
            let _ = app_handle.emit("keyframe-requested", ());
        }
        ServerMessage::Pong { timestamp } => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let rtt = now.saturating_sub(timestamp);
            let _ = app_handle.emit("ping-rtt", serde_json::json!({ "rtt_ms": rtt }));
        }
        ServerMessage::Error { code, message } => {
            warn!("Server error message (code {}): {}", code, message);
            let _ = app_handle.emit(
                "server-error",
                serde_json::json!({ "code": code, "message": message }),
            );
        }
        _ => {}
    }
}

/// Route an incoming media datagram (Audio → mixer, VideoDelta → frame assembler).
async fn route_datagram(bytes: Bytes, state: &AppState, app_handle: &tauri::AppHandle) {
    if bytes.len() < MEDIA_HEADER_SIZE {
        return;
    }
    let header = match MediaHeader::from_slice(&bytes[..MEDIA_HEADER_SIZE]) {
        Ok(h) => h,
        Err(_) => return,
    };
    let payload = bytes.slice(MEDIA_HEADER_SIZE..);

    match header.media_type {
        MediaType::Audio => {
            if !state.is_deafened.load(Ordering::Relaxed) {
                let mut mixer = state.mixer.lock().await;
                if let Some((peer_id, speaking)) =
                    mixer.on_audio_packet(&header, payload, Instant::now())
                {
                    let _ = app_handle.emit(
                        "user-speaking",
                        serde_json::json!({
                            "peer_id": peer_id,
                            "is_speaking": speaking
                        }),
                    );
                }
            }
        }
        MediaType::VideoDelta => {
            let mut asm = state.assembler.lock().await;
            if let Some(frame) = asm.push_fragment(&header, payload, Instant::now()) {
                if let Some(ch) = &*state.video_channel.read().await {
                    let _ = ch.send(VideoChunkPayload {
                        is_key: false,
                        data: frame.data.to_vec(),
                    });
                }
            }
        }
        _ => {}
    }
}
