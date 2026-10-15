//! QUIC session wiring: forwards `finch-client-core` session events to Tauri events,
//! the audio mixer and the screen-share frame assembler.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use finch_client_core::session::{connect, ConnectParams, SessionEvent};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::ServerMessage;
use quinn::Connection;
use tauri::Emitter;
use tokio::sync::mpsc;
use tracing::warn;

use crate::state::{AppState, VideoChunkPayload};

/// Establish QUIC connection and spawn the event forwarding loop.
pub async fn start_quic_session(
    server_addr: SocketAddr,
    hostname: &str,
    token: String,
    state: AppState,
    app_handle: tauri::AppHandle,
) -> anyhow::Result<Connection> {
    let (session, events) = connect(
        ConnectParams {
            quic_addr: server_addr,
            hostname: hostname.to_string(),
            token,
        },
        state.saved_fingerprints.clone(),
    )
    .await?;

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

    tokio::spawn(forward_events(events, state, app_handle));

    Ok(session.connection)
}

async fn forward_events(
    mut events: mpsc::Receiver<SessionEvent>,
    state: AppState,
    app_handle: tauri::AppHandle,
) {
    while let Some(event) = events.recv().await {
        match event {
            SessionEvent::Control(msg) => emit_control(msg, &app_handle),
            SessionEvent::Datagram(bytes) => route_datagram(bytes, &state, &app_handle).await,
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
            SessionEvent::ConnectionLost(reason) => {
                let _ = app_handle.emit("connection-lost", reason);
            }
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
