//! QUIC session wiring: forwards `finch-client-core` session events to Tauri events,
//! the audio mixer and the screen-share viewer, and reconnects after a drop.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use finch_client_core::error::ClientError;
use finch_client_core::session::{
    connect, reconnect_with_progress, Backoff, ConnectParams, Session, SessionEvent,
};
use finch_client_core::screen::ScreenViewer;
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use quinn::Connection;
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::state::AppState;

/// How long a quit may take in total; the app exits after this whatever the network does.
const QUIT_BOUND: Duration = Duration::from_millis(1500);
/// How long leaving the voice room may take before the connection is closed anyway.
const QUIT_LEAVE_BOUND: Duration = Duration::from_millis(1000);
/// Gives the queued LeaveVoice time to reach the wire before the connection closes.
const QUIT_LEAVE_FLUSH: Duration = Duration::from_millis(100);
/// Gives the connection close time to leave the endpoint before the process exits.
const QUIT_CLOSE_FLUSH: Duration = Duration::from_millis(250);

/// Quit path (tray Quit, Windows log-off, updater restart): leaves the voice room and closes the
/// QUIC session, so the server drops us at once instead of after its 15 s idle timeout. Best effort
/// and bounded by `QUIT_BOUND`, whatever the network or a stuck voice transition does.
pub async fn shutdown_session(state: &AppState) {
    state.game_activity.lock().await.disconnect();
    let routine = async {
        // Stop any reconnect first: the close below must not bring the session back. Lock-free on
        // purpose, so a stuck voice transition cannot hold this up.
        state.session_generation.fetch_add(1, Ordering::SeqCst);
        *state.last_connect.write().await = None;
        if state.current_voice_channel.read().await.is_some() {
            if tokio::time::timeout(QUIT_LEAVE_BOUND, crate::commands::voice::leave_room(state)).await.is_err() {
                warn!("Leaving the voice room timed out while quitting");
            }
            tokio::time::sleep(QUIT_LEAVE_FLUSH).await;
        }
        let connection = state.quic_connection.write().await.take();
        if let Some(connection) = connection {
            connection.close(0u32.into(), b"quit");
            tokio::time::sleep(QUIT_CLOSE_FLUSH).await;
        }
    };
    if tokio::time::timeout(QUIT_BOUND, routine).await.is_err() {
        warn!("Quitting timed out before the session was closed");
    }
}

async fn supersede_session(state: &AppState) -> u64 {
    state.game_activity.lock().await.disconnect();
    let _transition = state.voice_transition.lock().await;
    state.music.set_room(None);
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
    *state.mixer.lock().await = finch_audio::AudioMixer::new();
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

/// How often the session token is renewed while signed in.
const TOKEN_REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Renew the session token every 6 h while `generation` is current. Failures are retried next tick.
pub fn spawn_token_refresher(state: AppState, generation: u64) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TOKEN_REFRESH_INTERVAL).await;
            if state.session_generation.load(Ordering::SeqCst) != generation { return; }
            if let Err(e) = refresh_token_once(&state, generation).await {
                warn!("Token refresh failed, retrying later: {e}");
            }
        }
    });
}

/// One refresh: new token into the REST client, `auth_token`, reconnect params and the store.
pub async fn refresh_token_once(state: &AppState, generation: u64) -> Result<(), String> {
    let api = state.api.read().await.clone().ok_or("not signed in")?;
    let auth = api.refresh().await.map_err(|e| e.to_string())?;
    if state.session_generation.load(Ordering::SeqCst) != generation { return Ok(()); }
    if let Some(api) = state.api.write().await.as_mut() { api.set_token(auth.token.clone()); }
    *state.auth_token.write().await = Some(auth.token.clone());
    if let Some(params) = state.last_connect.write().await.as_mut() { params.token = auth.token.clone(); }
    if let Some(key) = state.signed_in_key.read().await.clone() {
        state.token_store.save(&key, &auth.token)?;
    }
    Ok(())
}

/// Make `session` the active one in the app state and tell the UI.
async fn activate_session(session: &Session, state: &AppState, app_handle: &tauri::AppHandle) {
    let generation = state.session_generation.load(Ordering::SeqCst);
    state.game_activity.lock().await.connect(
        generation, session.session_peer_id, session.user_id, &session.initial_state, session.control_tx.clone(),
    );
    if session.initial_state.features.iter().any(|feature| feature == "activity") {
        crate::activity::spawn_polling(state.clone(), generation, session.session_peer_id);
    }
    // Peer ids and packet sequences belong to this session, not the previous server.
    *state.mixer.lock().await = finch_audio::AudioMixer::new();
    state
        .session_peer_id
        .store(session.session_peer_id, Ordering::SeqCst);
    *state.control_tx.write().await = Some(session.control_tx.clone());
    *state.quic_connection.write().await = Some(session.connection.clone());
    let _ = app_handle.emit(
        "authenticated",
        serde_json::json!({
            "session_generation": generation,
            "session_peer_id": session.session_peer_id,
            "user_id": session.user_id,
            "channels": session.channels,
            "initial_state": session.initial_state
        }),
    );
}

/// Disconnect cancels pending first-frame capture as well as installed workers.
async fn stop_share_on_connection_loss(state: &AppState, generation: u64) -> bool {
    let _transition = state.voice_transition.lock().await;
    if state.session_generation.load(Ordering::SeqCst) != generation { return false; }
    state.music.suspend();
    state.stop_screen_share();
    true
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
        if !stop_share_on_connection_loss(&state, generation).await { return; }
        state.game_activity.lock().await.disconnect();
        *state.mixer.lock().await = finch_audio::AudioMixer::new();
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
            SessionEvent::Control(ServerMessage::MusicState { channel_id, state: room_state, server_ms }) => {
                let _transition = state.voice_transition.lock().await;
                if state.session_generation.load(Ordering::SeqCst) != generation { return None; }
                if let Some(api) = state.api.read().await.clone() {
                    if let Ok(cache) = app_handle.path().app_cache_dir() {
                        let app = app_handle.clone();
                        let source = state.clone();
                        state.music.receive(channel_id, room_state.clone(), server_ms, state.ping_rtt_ms.load(Ordering::Relaxed), api, cache.join("music"), std::sync::Arc::new(move |message| {
                            if source.session_generation.load(Ordering::SeqCst) == generation {
                                let _ = app.emit("music-error", serde_json::json!({ "message": message }));
                            }
                        }));
                    }
                }
                emit_control(ServerMessage::MusicState { channel_id, state: room_state, server_ms }, app_handle);
            }
            SessionEvent::Control(msg) => handle_control(state, msg, |msg| emit_control(msg, app_handle)).await,
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
        r = reconnect_with_progress(params, state.saved_fingerprints.clone(), Backoff::default(), state.reconnect_wake.clone(), |attempt, wait| {
            let _ = app_handle.emit("reconnect-attempt", serde_json::json!({ "attempt": attempt, "retry_in_secs": wait.as_secs() }));
        }) => r,
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
            activate_session(&session, state, app_handle).await;
            if restore_voice_membership(state, &session, generation).await {
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

async fn restore_voice_membership(state: &AppState, session: &Session, generation: u64) -> bool {
    let _transition = state.voice_transition.lock().await;
    if state.session_generation.load(Ordering::SeqCst) != generation { return false; }
    let channel = (*state.current_voice_channel.read().await).and_then(|short_id|
        session.channels.iter().find(|channel| channel.short_id == short_id).map(|channel| channel.id));
    let Some(channel_id) = channel else { return false; };
    if session.control_tx.send(ClientMessage::JoinVoice { channel_id }).await.is_err() { return false; }
    state.music.set_room(Some(channel_id));
    let _ = session.control_tx.send(ClientMessage::UpdateVoiceState {
        muted: state.is_muted.load(Ordering::SeqCst),
        deafened: state.is_deafened.load(Ordering::SeqCst),
    }).await;
    true
}

fn emit_control(msg: ServerMessage, app_handle: &tauri::AppHandle) {
    match msg {
        ServerMessage::FlockCall { from_user_id, from_name, channel_id } => {
            let _ = app_handle.emit("flock-call", serde_json::json!({ "from_user_id": from_user_id, "from_name": from_name, "channel_id": channel_id }));
        }
        ServerMessage::SoundPlayed { sound_id, user_id } => {
            let _ = app_handle.emit("sound-played", serde_json::json!({ "sound_id": sound_id, "user_id": user_id }));
        }
        ServerMessage::ActivityChanged { user_id, game } => {
            let _ = app_handle.emit("activity-changed", serde_json::json!({ "user_id": user_id, "game": game }));
        }
        ServerMessage::MusicState { channel_id, state, server_ms } => {
            let _ = app_handle.emit("music-state", serde_json::json!({ "channel_id": channel_id, "state": state, "server_ms": server_ms }));
        }
        ServerMessage::NewMessage {
            message_id,
            channel_id,
            author_id,
            author_name,
            content,
            attachments,
            timestamp,
            client_msg_id,
            reply_to,
            mentions,
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
                    "reply_to": reply_to,
                    "mentions": mentions,
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
        ServerMessage::ScreenShareStopped { peer_id, reason } => {
            let _ = app_handle.emit(
                "screen-share-stopped",
                serde_json::json!({ "peer_id": peer_id, "reason": reason }),
            );
        }
        ServerMessage::ServerMuteChanged { peer_id, server_muted } => {
            let _ = app_handle.emit("server-mute-changed", serde_json::json!({ "peer_id": peer_id, "server_muted": server_muted }));
        }
        ServerMessage::VoiceKicked { channel_id } => {
            let _ = app_handle.emit("voice-kicked", serde_json::json!({ "channel_id": channel_id }));
        }
        ServerMessage::VoiceMoved { channel_id } => {
            let _ = app_handle.emit("voice-moved", serde_json::json!({ "channel_id": channel_id }));
        }
        ServerMessage::JoinSoundChanged { user_id } => {
            let _ = app_handle.emit("join-sound-changed", serde_json::json!({ "user_id": user_id }));
        }
        ServerMessage::AvatarChanged { user_id, avatar_url } => {
            let _ = app_handle.emit("avatar-changed", serde_json::json!({ "user_id": user_id, "avatar_url": avatar_url }));
        }
        ServerMessage::PresenceUpdate { user_id, status, status_text } => {
            let _ = app_handle.emit(
                "presence-update",
                serde_json::json!({
                    "user_id": user_id,
                    "status": status,
                    "status_text": status_text,
                }),
            );
        }
        ServerMessage::ChannelCreated { channel } => {
            let _ = app_handle.emit("channel-created", serde_json::json!({ "channel": channel }));
        }
        ServerMessage::MessageEdited { message_id, channel_id, content, edited_at } => {
            let _ = app_handle.emit(
                "message-edited",
                serde_json::json!({ "message_id": message_id, "channel_id": channel_id, "content": content, "edited_at": edited_at }),
            );
        }
        ServerMessage::MessageDeleted { message_id, channel_id } => {
            let _ = app_handle.emit("message-deleted", serde_json::json!({ "message_id": message_id, "channel_id": channel_id }));
        }
        ServerMessage::ReactionChanged { message_id, channel_id, user_id, emoji, added } => {
            let _ = app_handle.emit(
                "reaction-changed",
                serde_json::json!({ "message_id": message_id, "channel_id": channel_id, "user_id": user_id, "emoji": emoji, "added": added }),
            );
        }
        ServerMessage::UserTyping { channel_id, user_id, username } => {
            let _ = app_handle.emit("user-typing", serde_json::json!({ "channel_id": channel_id, "user_id": user_id, "username": username }));
        }
        ServerMessage::UserProfileChanged { user_id, display_name } => {
            let _ = app_handle.emit("user-profile-changed", serde_json::json!({ "user_id": user_id, "display_name": display_name }));
        }
        ServerMessage::ChannelUpdated { channel } => {
            let _ = app_handle.emit("channel-updated", serde_json::json!({ "channel": channel }));
        }
        ServerMessage::ChannelDeleted { channel_id } => {
            let _ = app_handle.emit("channel-deleted", serde_json::json!({ "channel_id": channel_id }));
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

/// Accept voice only from the current room, atomically with room transitions.
async fn route_voice_audio(state: &AppState, header: &MediaHeader, payload: Bytes) -> Option<(u32, bool)> {
    // Keep this guard through insertion: a room transition then clears everything
    // accepted before the transition, and packets arriving later fail this check.
    let channel = state.current_voice_channel.read().await;
    if *channel != Some(header.channel_id) { return None; }
    if state.is_deafened.load(Ordering::Relaxed) { return None; }
    state.mixer.lock().await.on_audio_packet(header, payload, Instant::now())
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
            if let Some((peer_id, speaking)) = route_voice_audio(state, &header, payload).await {
                let _ = app_handle.emit(
                    "user-speaking",
                    serde_json::json!({
                        "peer_id": peer_id,
                        "is_speaking": speaking
                    }),
                );
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

/// Bookkeeping for a control message (mixer and watched share), then the event goes to the UI.
async fn handle_control(state: &AppState, msg: ServerMessage, emit: impl FnOnce(ServerMessage)) {
    if let ServerMessage::PresenceUpdate { user_id, status, .. } = &msg {
        state.game_activity.lock().await.set_status(*user_id, *status).await;
    }
    match &msg {
        ServerMessage::Pong { timestamp } => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis() as u64);
            state.ping_rtt_ms.store(now.saturating_sub(*timestamp), Ordering::Relaxed);
        }
        ServerMessage::UserLeftVoice { peer_id, .. } | ServerMessage::UserJoinedVoice { peer_id, .. } => {
            state.mixer.lock().await.remove_speaker(*peer_id);
        }
        ServerMessage::ChannelUpdated { channel } => {
            state.channel_bitrates.write().await.insert(channel.short_id, channel.bitrate_kbps);
            // An admin's change applies to the room we are in; the audio worker picks it up in place.
            if *state.current_voice_channel.read().await == Some(channel.short_id) {
                state.voice_bitrate_kbps.store(
                    crate::audio_engine::voice_bitrate_kbps(Some(channel.bitrate_kbps)),
                    Ordering::Relaxed,
                );
            }
        }
        _ => {}
    }
    track_watched_share(state, &msg).await;
    emit(msg);
}

/// Watching ends when the share we watch ends: the sharer stopping, or the same share starting
/// again under a new peer id. Nothing follows a new share; the user has to press "Watch" again.
async fn track_watched_share(state: &AppState, msg: &ServerMessage) {
    match msg {
        ServerMessage::ScreenShareStopped { peer_id, .. } => {
            state.mixer.lock().await.retire_stream(finch_audio::screen_audio_key(*peer_id));
            if state.viewing_peer.load(Ordering::SeqCst) == *peer_id {
                state.stop_watching().await;
            }
        }
        ServerMessage::ScreenShareStarted { user_id, peer_id, .. } => {
            let watched = *state.viewing_user.read().await == Some(*user_id);
            if watched && state.viewing_peer.load(Ordering::SeqCst) != *peer_id {
                state.stop_watching().await;
            }
        }
        _ => {}
    }
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

/// One raw binary IPC message: byte 0 is the flags (bit 0 = key frame), the rest is the access unit.
fn video_message(is_key: bool, data: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(data.len() + 1);
    message.push(u8::from(is_key));
    message.extend_from_slice(data);
    message
}

async fn send_video(state: &AppState, is_key: bool, data: Bytes) {
    if let Some(channel) = &*state.video_channel.read().await {
        let _ = channel.send(tauri::ipc::InvokeResponseBody::Raw(video_message(is_key, &data)));
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

/// Wall-clock gap beyond which the PC is taken to have slept.
const SLEEP_GAP: Duration = Duration::from_secs(30);

/// True iff `wall` exceeds the `expected` tick by more than the sleep gap.
fn slept(expected: Duration, wall: Duration) -> bool {
    wall > expected + SLEEP_GAP
}

/// Notice a PC that went to sleep (the wall clock jumps while the timer was paused) and
/// make a pending reconnect try again at once instead of finishing its backoff wait.
pub fn spawn_sleep_watch(state: AppState) {
    tauri::async_runtime::spawn(async move {
        let tick = Duration::from_secs(5);
        let mut last = SystemTime::now();
        loop {
            tokio::time::sleep(tick).await;
            let now = SystemTime::now();
            let wall = now.duration_since(last).unwrap_or_default();
            last = now;
            if slept(tick, wall) {
                info!("The PC woke up after {wall:?}; reconnecting at once");
                state.reconnect_wake.notify_one();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use finch_protocol::models::{ShareEndReason, VideoCodec};

    #[test]
    fn video_message_flags_and_payload() {
        assert_eq!(video_message(true, &[0xAA, 0x00, 0xFF]), vec![1, 0xAA, 0x00, 0xFF]);
        assert_eq!(video_message(false, &[0x01, 0x02]), vec![0, 0x01, 0x02]);
        assert_eq!(video_message(true, &[]), vec![1]);
        assert_eq!(video_message(false, &[]), vec![0]);
    }
    use finch_testbed::{client::TestClient, server::TestServer};

    #[test]
    fn slept_detects_large_gap() {
        let tick = Duration::from_secs(5);
        assert!(!slept(tick, Duration::from_secs(5)));
        assert!(slept(tick, Duration::from_secs(60)));
    }

    #[tokio::test]
    async fn connection_loss_cancels_pending_screen_preflight_without_touching_a_new_session() {
        let state = AppState::new();
        let reservation = state.reserve_screen_share().unwrap();
        assert!(stop_share_on_connection_loss(&state, 0).await);
        assert!(state.with_screen_share(reservation.generation(), || Ok(())).is_err());
        let current = state.reserve_screen_share().unwrap();
        state.session_generation.store(1, Ordering::SeqCst);
        assert!(!stop_share_on_connection_loss(&state, 0).await);
        assert!(state.with_screen_share(current.generation(), || Ok(())).is_ok());
    }

    #[tokio::test]
    async fn replacing_session_waits_for_voice_transition() {
        let state = AppState::new();
        let transition = state.voice_transition.lock().await;
        let cloned = state.clone();
        let replacing = tokio::spawn(async move { supersede_session(&cloned).await });
        tokio::task::yield_now().await;
        assert_eq!(state.session_generation.load(Ordering::SeqCst), 0);
        assert!(!replacing.is_finished());
        drop(transition);
        assert_eq!(replacing.await.unwrap(), 1);
    }

    #[tokio::test]
    async fn pending_leave_prevents_reconnect_from_restoring_stale_membership() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let client = TestClient::register(&server, "rejoinrace").await?;
        let mut session = client.session.clone();
        let (tx, mut rx) = mpsc::channel(8);
        session.control_tx = tx;
        let state = AppState::new();
        *state.current_voice_channel.write().await = Some(client.channel(finch_protocol::models::ChannelType::Voice).1);
        let transition = state.voice_transition.lock().await;
        let cloned = state.clone();
        let restoring = tokio::spawn(async move { restore_voice_membership(&cloned, &session, 0).await });
        assert!(tokio::time::timeout(Duration::from_millis(50), rx.recv()).await.is_err(),
            "automatic rejoin must wait until the explicit leave has completed");
        *state.current_voice_channel.write().await = None;
        drop(transition);
        assert!(!restoring.await?);
        assert!(rx.try_recv().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn delayed_audio_cannot_enter_after_leave_or_from_another_room() {
        let state = AppState::new();
        let mut encoder = finch_audio::OpusVoiceEncoder::new(64).unwrap();
        let payload = encoder.encode(&[0.1; finch_audio::FRAME_SIZE_48K]).unwrap();
        let header = MediaHeader::new_audio(1, 7, 0, 0, false, false);
        route_voice_audio(&state, &header, payload.clone()).await;
        assert!(state.mixer.lock().await.loss_rate(7).is_none(), "left room must stay silent");
        *state.current_voice_channel.write().await = Some(2);
        route_voice_audio(&state, &header, payload.clone()).await;
        assert!(state.mixer.lock().await.loss_rate(7).is_none(), "old room cannot refill the mixer");
        let header = MediaHeader::new_audio(2, 7, 0, 0, false, false);
        assert_eq!(route_voice_audio(&state, &header, payload).await, Some((7, true)));
    }

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
    async fn aud018_watching_ends_with_the_share_and_a_restarted_share_is_not_followed() {
        let state = AppState::new();
        let (tx, mut rx) = mpsc::channel(8);
        *state.control_tx.write().await = Some(tx);
        let alice = uuid::Uuid::new_v4();
        *state.viewing_user.write().await = Some(alice);
        state.viewing_peer.store(5, Ordering::SeqCst);
        let alice_sound = MediaHeader::new_audio(1, 5, 0, 0, false, false);
        assert!(is_watching(&state, &alice_sound), "alice's sound is accepted while watched");

        let mut emitted = Vec::new();
        handle_control(&state, ServerMessage::ScreenShareStopped { peer_id: 5, reason: ShareEndReason::Stopped }, |m| emitted.push(m)).await;
        assert_eq!(emitted.len(), 1, "the UI still learns that the share stopped");
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 0);
        assert!(state.viewing_user.read().await.is_none());
        assert!(!is_watching(&state, &alice_sound), "no sound of the stopped share is accepted");

        // Alice starts sharing again as peer 7: nobody subscribes, the watch stays off.
        handle_control(&state, ServerMessage::ScreenShareStarted {
            user_id: alice, peer_id: 7, codec: VideoCodec::H264, width: 64, height: 64, fps: 30,
        }, |m| emitted.push(m)).await;
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 0);
        assert!(state.viewing_user.read().await.is_none());
        assert!(!is_watching(&state, &MediaHeader::new_audio(1, 7, 0, 0, false, false)));
        assert!(rx.try_recv().is_err(), "a new share is only watched after a click");
    }

    #[tokio::test]
    async fn aud018_another_sharers_stop_leaves_the_watch_alone() {
        let state = AppState::new();
        let carol = uuid::Uuid::new_v4();
        *state.viewing_user.write().await = Some(carol);
        state.viewing_peer.store(9, Ordering::SeqCst);
        handle_control(&state, ServerMessage::ScreenShareStopped { peer_id: 5, reason: ShareEndReason::Stopped }, |_| {}).await;
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 9);
        assert_eq!(*state.viewing_user.read().await, Some(carol));
        assert!(is_watching(&state, &MediaHeader::new_audio(1, 9, 0, 0, false, false)));
    }

    #[tokio::test]
    async fn aud018_a_restarted_share_under_a_new_peer_ends_the_watch() {
        let state = AppState::new();
        let alice = uuid::Uuid::new_v4();
        *state.viewing_user.write().await = Some(alice);
        state.viewing_peer.store(5, Ordering::SeqCst);
        handle_control(&state, ServerMessage::ScreenShareStarted {
            user_id: alice, peer_id: 7, codec: VideoCodec::H264, width: 64, height: 64, fps: 30,
        }, |_| {}).await;
        assert_eq!(state.viewing_peer.load(Ordering::SeqCst), 0);
        assert!(state.viewing_user.read().await.is_none());
    }

    #[tokio::test]
    async fn aud018_subscribing_again_without_a_watched_share_does_nothing() {
        let state = AppState::new();
        let (tx, mut rx) = mpsc::channel(8);
        *state.control_tx.write().await = Some(tx);
        resubscribe(&state).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn quitting_tells_the_room_at_once_instead_of_after_the_idle_timeout() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let mut alice = TestClient::register(&server, "quitalice").await?;
        let mut bob = TestClient::register(&server, "quitbob").await?;
        let (_, alice_short) = alice.join_voice().await?;
        bob.join_voice().await?;
        let alice_peer = alice.session.session_peer_id;
        let alice_user = alice.session.user_id;
        let state = AppState::new();
        *state.control_tx.write().await = Some(alice.session.control_tx.clone());
        *state.quic_connection.write().await = Some(alice.session.connection.clone());
        *state.current_voice_channel.write().await = Some(alice_short);
        *state.last_connect.write().await = Some(ConnectParams {
            quic_addr: server.quic_addr,
            hostname: "localhost".to_string(),
            token: String::new(),
            require_known_fingerprint: false,
        });
        let generation = state.session_generation.load(Ordering::SeqCst);

        let started = Instant::now();
        shutdown_session(&state).await;
        bob.expect_control(Duration::from_secs(1), |m| match m {
            ServerMessage::UserLeftVoice { peer_id, .. } if *peer_id == alice_peer => Some(()),
            _ => None,
        })
        .await?;
        assert!(started.elapsed() < Duration::from_secs(1), "quit must not wait for the 15 s idle timeout");
        assert!(state.quic_connection.read().await.is_none());
        assert_ne!(state.session_generation.load(Ordering::SeqCst), generation, "quit must stop the reconnect loop");
        assert!(state.last_connect.read().await.is_none(), "quit must forget the session to reconnect to");
        bob.expect_no_control(Duration::from_secs(1), |m| {
            matches!(m, ServerMessage::PresenceUpdate { user_id, status: finch_protocol::models::UserStatus::Online, .. } if *user_id == alice_user)
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn quitting_without_a_connection_returns_within_the_bound() {
        let state = AppState::new();
        let started = Instant::now();
        shutdown_session(&state).await;
        assert!(started.elapsed() < QUIT_BOUND, "no connection must not block quitting");
    }

    #[tokio::test]
    async fn quitting_still_closes_the_connection_when_a_voice_transition_is_stuck() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let client = TestClient::register(&server, "quitstuck").await?;
        let state = AppState::new();
        *state.quic_connection.write().await = Some(client.session.connection.clone());
        *state.control_tx.write().await = Some(client.session.control_tx.clone());
        *state.current_voice_channel.write().await = Some(1);
        let stuck = state.voice_transition.lock().await;

        let started = Instant::now();
        shutdown_session(&state).await;
        assert!(started.elapsed() < QUIT_BOUND, "a stuck transition must not hold quitting");
        drop(stuck);
        tokio::time::timeout(Duration::from_millis(500), client.session.connection.closed())
            .await
            .expect("quit must close the QUIC connection even when the leave cannot finish");
        Ok(())
    }
}
