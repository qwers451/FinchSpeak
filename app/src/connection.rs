//! QUIC session wiring: forwards `finch-client-core` session events to Tauri events,
//! the audio mixer and the screen-share viewer, and reconnects after a drop.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use finch_client_core::error::ClientError;
use finch_client_core::session::{
    connect, reconnect_with_backoff, Backoff, ConnectParams, Session, SessionEvent,
};
use finch_client_core::screen::ScreenViewer;
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use quinn::Connection;
use tauri::Emitter;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::state::{AppState, VideoChunkPayload};

async fn supersede_session(state: &AppState) -> u64 {
    let generation = state.session_generation.fetch_add(1, Ordering::SeqCst) + 1;
    *state.last_connect.write().await = None;
    if let Some(connection) = state.quic_connection.write().await.take() {
        connection.close(0u32.into(), b"session replaced");
    }
    *state.control_tx.write().await = None;
    state.audio_engine.lock().await.stop();
    state.stop_screen_share();
    state.stop_watching().await;
    *state.current_voice_channel.write().await = None;
    generation
}

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
    let generation = supersede_session(&state).await;
    let connected = connect(params.clone(), state.saved_fingerprints.clone()).await;
    if state.session_generation.load(Ordering::SeqCst) != generation {
        if let Ok((session, _)) = &connected { session.connection.close(0u32.into(), b"session replaced"); }
        anyhow::bail!("Session was replaced while connecting");
    }
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
    tokio::spawn(screen_viewer_ticker(state.clone(), generation));
    tokio::spawn(run_session(events, state, app_handle, generation));
    Ok(connection)
}

/// Make `session` the active one in the app state and tell the UI.
async fn activate_session(session: &Session, state: &AppState, app_handle: &tauri::AppHandle) {
    state
        .session_peer_id
        .store(session.session_peer_id, Ordering::SeqCst);
    *state.control_tx.write().await = Some(session.control_tx.clone());
    *state.quic_connection.write().await = Some(session.connection.clone());
    let _ = app_handle.emit(
        "authenticated",
        serde_json::json!({
            "session_peer_id": session.session_peer_id,
            "user_id": session.user_id,
            "channels": session.channels,
            "initial_state": session.initial_state
        }),
    );
}

/// Forward session events; after a drop, reconnect and continue with the new session.
async fn run_session(
    mut events: mpsc::Receiver<SessionEvent>,
    state: AppState,
    app_handle: tauri::AppHandle,
    generation: u64,
) {
    loop {
        let Some(reason) = forward_events(&mut events, &state, &app_handle, generation).await else {
            return;
        };
        if state.session_generation.load(Ordering::SeqCst) != generation { return; }
        let _ = app_handle.emit("connection-lost", reason);
        match reconnect(&state, &app_handle, generation).await {
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
    generation: u64,
) -> Option<String> {
    while let Some(event) = events.recv().await {
        if state.session_generation.load(Ordering::SeqCst) != generation { return None; }
        match event {
            SessionEvent::Control(ServerMessage::KeyframeRequested) => {
                // A viewer needs a new key frame of the screen we are sharing.
                if let Ok(slot) = state.screen_sharer.lock() {
                    if let Some(sharer) = slot.as_ref() {
                        sharer.request_keyframe();
                    }
                }
            }
            SessionEvent::Control(msg) => {
                if let ServerMessage::ScreenShareStarted { user_id, peer_id, .. } = &msg {
                    follow_watched_share(state, *user_id, *peer_id).await;
                }
                emit_control(msg, app_handle)
            }
            SessionEvent::Datagram(bytes) => route_datagram(bytes, state, app_handle).await,
            SessionEvent::UniStreamStarted { header } => {
                if header.media_type == MediaType::VideoKey && is_watching(state, &header) {
                    state.screen_viewer.lock().await.keyframe_stream_started(Instant::now());
                }
            }
            SessionEvent::UniStream { header, payload } => {
                if header.media_type == MediaType::VideoKey && is_watching(state, &header) {
                    let frame = state.screen_viewer.lock().await.on_keyframe(&header, payload, Instant::now());
                    send_video(state, true, frame.data).await;
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
    generation: u64,
) -> Option<mpsc::Receiver<SessionEvent>> {
    if state.session_generation.load(Ordering::SeqCst) != generation { return None; }
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
            if state.session_generation.load(Ordering::SeqCst) != generation {
                session.connection.close(0u32.into(), b"session replaced");
                return None;
            }
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
                let _ = session.control_tx.send(ClientMessage::UpdateVoiceState {
                    muted: state.is_muted.load(Ordering::SeqCst),
                    deafened: state.is_deafened.load(Ordering::SeqCst),
                }).await;
                // Same control stream, so this is processed after the rejoin above.
                *state.screen_viewer.lock().await = ScreenViewer::new();
                resubscribe(state).await;
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
            client_msg_id,
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
                    "client_msg_id": client_msg_id,
                }),
            );
        }
        ServerMessage::MessageRejected {
            client_msg_id,
            code,
            message,
        } => {
            let _ = app_handle.emit(
                "message-rejected",
                serde_json::json!({
                    "client_msg_id": client_msg_id,
                    "code": code,
                    "message": message,
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

/// Route an incoming media datagram (Audio → mixer, VideoDelta → screen viewer).
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
        MediaType::ScreenAudio if is_watching(state, &header) => {
            if !state.is_deafened.load(Ordering::Relaxed) {
                state.mixer.lock().await.on_screen_audio_packet(&header, payload, Instant::now());
            }
        }
        MediaType::VideoDelta if is_watching(state, &header) => {
            let frame = state.screen_viewer.lock().await.on_delta_fragment(&header, payload, Instant::now());
            if let Some(frame) = frame {
                send_video(state, false, frame.data).await;
            }
        }
        _ => {}
    }
}

/// The share we watch (re)started, perhaps under a new peer id: follow it and subscribe again.
async fn follow_watched_share(state: &AppState, user_id: uuid::Uuid, peer_id: u32) {
    if *state.viewing_user.read().await != Some(user_id) {
        return;
    }
    state.viewing_peer.store(peer_id, Ordering::SeqCst);
    *state.screen_viewer.lock().await = ScreenViewer::new();
    resubscribe(state).await;
}

/// Ask the server for the share we are watching again (after a reconnect or a restarted share).
async fn resubscribe(state: &AppState) {
    let Some(user_id) = *state.viewing_user.read().await else { return };
    if let Some(tx) = state.control_tx.read().await.clone() {
        let _ = tx.send(ClientMessage::SubscribeScreenShare { target_user_id: user_id }).await;
    }
}

/// Only media from the sharer we subscribed to is shown.
fn is_watching(state: &AppState, header: &MediaHeader) -> bool {
    let peer = state.viewing_peer.load(Ordering::SeqCst);
    peer != 0 && header.sender_peer_id == peer
}

async fn send_video(state: &AppState, is_key: bool, data: Bytes) {
    if let Some(channel) = &*state.video_channel.read().await {
        let _ = channel.send(VideoChunkPayload { is_key, data: data.to_vec() });
    }
}

/// While watching a screen share: drop incomplete frames and ask the sharer for a key frame
/// whenever the picture cannot be decoded (a new viewer, a lost packet).
async fn screen_viewer_ticker(state: AppState, generation: u64) {
    let mut interval = tokio::time::interval(Duration::from_millis(50));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if state.session_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let peer = state.viewing_peer.load(Ordering::SeqCst);
        if peer == 0 {
            continue;
        }
        let now = Instant::now();
        let ask = {
            let mut viewer = state.screen_viewer.lock().await;
            viewer.tick(now);
            viewer.take_keyframe_request(now)
        };
        if ask {
            let tx = state.control_tx.read().await.clone();
            if let Some(tx) = tx {
                let _ = tx.send(ClientMessage::RequestKeyframe { target_peer_id: peer }).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use finch_testbed::{client::TestClient, server::TestServer};

    #[tokio::test]
    async fn aud031_replacing_session_closes_previous_quic_connection() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let client = TestClient::register(&server, "alice").await?;
        let state = AppState::new();
        *state.quic_connection.write().await = Some(client.session.connection.clone());
        *state.control_tx.write().await = Some(client.session.control_tx.clone());
        supersede_session(&state).await;
        tokio::time::timeout(Duration::from_millis(500), client.session.connection.closed())
            .await.expect("superseded QUIC session must close explicitly");
        assert!(state.quic_connection.read().await.is_none());
        assert!(state.control_tx.read().await.is_none());
        Ok(())
    }

    struct SolidSource;

    impl finch_client_core::screen::FrameSource for SolidSource {
        fn next_frame(&mut self) -> Result<finch_screen::RawFrame, String> {
            Ok(finch_screen::RawFrame::new(
                64,
                64,
                finch_screen::PixelFormat::Bgra,
                Bytes::from(vec![90u8; 64 * 64 * 4]),
            ))
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn aud018_replacing_session_stops_screen_share_and_viewing() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let client = TestClient::register(&server, "alice").await?;
        let state = AppState::new();
        let sharer = finch_client_core::screen::ScreenSharer::start(
            client.session.connection.clone(),
            tokio::runtime::Handle::current(),
            1,
            client.session.session_peer_id,
            finch_client_core::screen::ShareConfig::default(),
            Box::new(SolidSource),
            |_| {},
        );
        *state.screen_sharer.lock().unwrap() = Some(sharer);
        state.viewing_peer.store(7, Ordering::SeqCst);

        supersede_session(&state).await;

        assert!(state.screen_sharer.lock().unwrap().is_none(), "the capture thread must be stopped");
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 0);
        Ok(())
    }

    #[tokio::test]
    async fn aud018_viewer_follows_a_share_that_restarts_under_a_new_peer() {
        let state = AppState::new();
        let (tx, mut rx) = mpsc::channel(8);
        *state.control_tx.write().await = Some(tx);
        let alice = uuid::Uuid::new_v4();
        *state.viewing_user.write().await = Some(alice);
        state.viewing_peer.store(5, Ordering::SeqCst);

        // Someone else's share is none of our business.
        follow_watched_share(&state, uuid::Uuid::new_v4(), 99).await;
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 5);
        assert!(rx.try_recv().is_err());

        // Alice's share starts again, now as peer 7: follow it and subscribe again.
        follow_watched_share(&state, alice, 7).await;
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 7);
        assert!(matches!(
            rx.try_recv(),
            Ok(ClientMessage::SubscribeScreenShare { target_user_id }) if target_user_id == alice
        ));
    }

    #[tokio::test]
    async fn aud018_subscribing_again_without_a_watched_share_does_nothing() {
        let state = AppState::new();
        let (tx, mut rx) = mpsc::channel(8);
        *state.control_tx.write().await = Some(tx);
        resubscribe(&state).await;
        assert!(rx.try_recv().is_err());
    }
}
