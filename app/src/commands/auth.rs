//! Authentication and session management commands.

use finch_client_core::api::{resolve_quic_addr_with_port, ApiClient, AuthResponse, SessionDto};
use finch_client_core::error::ClientError;
use tauri::{AppHandle, Emitter, State};
use std::sync::atomic::Ordering;
use tracing::{error, info, warn};

use crate::connection::{spawn_token_refresher, start_quic_session};
use crate::credentials::credential_key;
use crate::state::{AppState, UserDto};

#[derive(serde::Serialize)]
pub struct PendingFingerprint {
    hostname: String,
    fingerprint: String,
}

#[derive(serde::Serialize)]
pub struct SessionSnapshot {
    session_generation: u64,
    connected: bool,
    user_id: String,
    session_peer_id: u32,
    current_voice_channel: Option<u32>,
    muted: bool,
    deafened: bool,
    pending_tofu: Option<PendingFingerprint>,
    initial_state: Option<finch_protocol::models::ServerState>,
}

#[tauri::command]
pub async fn get_session_snapshot(state: State<'_, AppState>) -> Result<Option<SessionSnapshot>, String> {
    use std::sync::atomic::Ordering;
    let generation = state.session_generation.load(Ordering::SeqCst);
    let Some(user) = state.current_user.read().await.clone() else { return Ok(None); };
    let connected = state.quic_connection.read().await.as_ref().is_some_and(|connection| connection.close_reason().is_none());
    let api = state.api.read().await.clone();
    let initial_state = if connected {
        let api = api.ok_or_else(|| "Connected session has no REST client".to_string())?;
        Some(api.server_state().await.map_err(|error| error.to_string())?)
    } else { None };
    if state.session_generation.load(Ordering::SeqCst) != generation { return Ok(None); }
    let pending_tofu = state.pending_fingerprint.read().await.clone()
        .map(|(hostname, fingerprint)| PendingFingerprint { hostname, fingerprint });
    Ok(Some(SessionSnapshot {
        session_generation: generation,
        connected, user_id: user.id,
        session_peer_id: state.session_peer_id.load(Ordering::SeqCst),
        current_voice_channel: *state.current_voice_channel.read().await,
        muted: state.is_muted.load(Ordering::SeqCst),
        deafened: state.is_deafened.load(Ordering::SeqCst),
        pending_tofu, initial_state,
    }))
}

/// Reports UI events (`tofu-required`, `tofu-mismatch`); a closure so the trust logic is testable.
type Emit<'a> = &'a (dyn Fn(&str, serde_json::Value) + Send + Sync);

/// REST client for `server_url` whose certificate has been confirmed by the user.
///
/// The TLS handshake is completed (without credentials) *before* any password is sent: an
/// unknown server raises `tofu-required` for the confirmation dialog, a changed certificate
/// is refused outright.
async fn trusted_api(
    server_url: &str,
    state: &AppState,
    app_handle: &AppHandle,
) -> Result<ApiClient, String> {
    trusted_api_with(server_url, state, &|name, payload| {
        let _ = app_handle.emit(name, payload);
    })
    .await
}

async fn trusted_api_with(server_url: &str, state: &AppState, emit: Emit<'_>) -> Result<ApiClient, String> {
    let mut api = ApiClient::with_tofu(server_url, state.saved_fingerprints.clone())
        .map_err(|e| format!("Invalid server address: {e}"))?;
    api.set_device_name(std::env::var("COMPUTERNAME").ok());
    match api.verify_trust().await {
        Ok(()) => Ok(api),
        Err(ClientError::UntrustedServer { hostname, fingerprint }) => {
            *state.pending_fingerprint.write().await = Some((hostname.clone(), fingerprint.clone()));
            emit("tofu-required", serde_json::json!({ "hostname": hostname, "fingerprint": fingerprint }));
            Err(format!(
                "Server certificate of {hostname} is not trusted yet - confirm its fingerprint and sign in again"
            ))
        }
        Err(ClientError::FingerprintMismatch { hostname, expected, actual }) => {
            emit("tofu-mismatch", serde_json::json!({ "hostname": hostname, "expected": expected, "actual": actual }));
            Err(format!(
                "The certificate of {hostname} changed since you trusted it. Someone may be intercepting the connection; do not sign in unless the server administrator confirms the change."
            ))
        }
        Err(e) => Err(format!("Cannot reach the server: {e}")),
    }
}

/// Remember (or not) the token of a fresh sign-in; sets `signed_in_key` for logout and refresh.
async fn remember_sign_in(state: &AppState, server_url: &str, username: &str, token: &str, remember: bool) {
    if !remember {
        // An older token of this account must not survive: the next start would sign in anyway.
        state.token_store.delete(&credential_key(server_url, username));
        *state.signed_in_key.write().await = None;
        return;
    }
    let key = credential_key(server_url, username);
    if let Err(e) = state.token_store.save(&key, token) {
        warn!("Could not remember the sign-in: {e}");
    }
    *state.signed_in_key.write().await = Some(key);
}

/// Forget the stored token of the current session (logout).
async fn forget_stored_token(state: &AppState) {
    if let Some(key) = state.signed_in_key.write().await.take() {
        state.token_store.delete(&key);
    }
}

/// State part of signing in: everything except the QUIC connection.
async fn apply_sign_in(mut api: ApiClient, auth: &AuthResponse, server_url: &str, state: &AppState) {
    api.set_token(auth.token.clone());
    *state.api.write().await = Some(api);
    *state.server_url.write().await = Some(server_url.to_string());
    *state.auth_token.write().await = Some(auth.token.clone());
    *state.current_user.write().await = Some(auth.user.clone());
}

/// Shared tail of login, register and resume: store the session and connect over QUIC.
async fn finish_sign_in(
    api: ApiClient,
    auth: AuthResponse,
    server_url: &str,
    state: &AppState,
    app_handle: AppHandle,
) -> Result<UserDto, String> {
    apply_sign_in(api, &auth, server_url, state).await;
    match resolve_quic_addr_with_port(server_url, auth.quic_port) {
        Ok((quic_addr, hostname)) => {
            *state.server_quic_addr.write().await = Some(quic_addr);
            if let Err(e) =
                start_quic_session(quic_addr, &hostname, auth.token.clone(), state.clone(), app_handle).await
            {
                error!("QUIC connection failed: {}", e);
                return Err(format!("Signed in, but the QUIC connection failed: {}", e));
            }
        }
        Err(e) => {
            error!("Failed to resolve QUIC address: {}", e);
            return Err(format!("Failed to resolve QUIC address: {}", e));
        }
    }
    spawn_token_refresher(state.clone(), state.session_generation.load(Ordering::SeqCst));
    Ok(auth.user)
}

#[tauri::command]
pub async fn login(
    server_url: String,
    username: String,
    password: String,
    remember: Option<bool>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<UserDto, String> {
    info!("Logging in to {} as {}", server_url, username);

    let api = trusted_api(&server_url, &state, &app_handle).await?;
    let auth_data: AuthResponse = api
        .login(&username, &password)
        .await
        .map_err(|e| format!("Login failed: {}", e))?;
    remember_sign_in(&state, &server_url, &username, &auth_data.token, remember.unwrap_or(true)).await;
    finish_sign_in(api, auth_data, &server_url, &state, app_handle).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn register(
    server_url: String,
    username: String,
    display_name: String,
    password: String,
    invite_code: Option<String>,
    remember: Option<bool>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<UserDto, String> {
    info!("Registering to {} as {}", server_url, username);

    let api = trusted_api(&server_url, &state, &app_handle).await?;
    let invite = invite_code.as_deref().map(str::trim).filter(|code| !code.is_empty());
    let registered = match invite {
        Some(code) => api.register_with_invite(&username, &password, &display_name, code).await,
        None => api.register(&username, &password, &display_name).await,
    };
    let auth_data: AuthResponse = registered.map_err(|e| format!("Registration failed: {}", e))?;
    remember_sign_in(&state, &server_url, &username, &auth_data.token, remember.unwrap_or(true)).await;
    finish_sign_in(api, auth_data, &server_url, &state, app_handle).await
}

/// Exchange the remembered token for a fresh one. `Ok(None)`: nothing stored, or the server
/// no longer accepts it (then it is deleted). A network failure keeps the token.
async fn refresh_stored_session(
    server_url: &str,
    username: &str,
    state: &AppState,
    emit: Emit<'_>,
) -> Result<Option<(ApiClient, AuthResponse)>, String> {
    let key = credential_key(server_url, username);
    let Some(token) = state.token_store.load(&key) else { return Ok(None) };
    let mut api = trusted_api_with(server_url, state, emit).await?;
    api.set_token(token);
    match api.refresh().await {
        Ok(auth) => {
            remember_sign_in(state, server_url, username, &auth.token, true).await;
            Ok(Some((api, auth)))
        }
        Err(ClientError::Http { status: 401, .. }) => {
            state.token_store.delete(&key);
            Ok(None)
        }
        Err(e) => Err(format!("Cannot reach the server: {e}")),
    }
}

/// Sign in again with the remembered token (no password). `Ok(None)`: the user must sign in.
#[tauri::command]
pub async fn resume_session(
    server_url: String,
    username: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<Option<UserDto>, String> {
    info!("Resuming session on {} as {}", server_url, username);
    let emit = |name: &str, payload: serde_json::Value| {
        let _ = app_handle.emit(name, payload);
    };
    let Some((api, auth)) = refresh_stored_session(&server_url, &username, &state, &emit).await? else {
        return Ok(None);
    };
    finish_sign_in(api, auth, &server_url, &state, app_handle).await.map(Some)
}

async fn signed_in_api(state: &AppState) -> Result<ApiClient, String> {
    state.api.read().await.clone().ok_or_else(|| "Not authenticated".to_string())
}

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionDto>, String> {
    signed_in_api(&state).await?.list_sessions().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn end_session(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    signed_in_api(&state).await?.end_session(&session_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn end_other_sessions(state: State<'_, AppState>) -> Result<(), String> {
    signed_in_api(&state).await?.end_other_sessions().await.map_err(|e| e.to_string())
}

/// First half of ending a session on this device: bumps the session generation first, so the token
/// refresher and any reconnect loop stop before anything else is torn down.
async fn stop_session_activity(state: &AppState) {
    state.game_activity.lock().await.disconnect();
    state.session_generation.fetch_add(1, Ordering::SeqCst);
    *state.last_connect.write().await = None;
    state.audio_engine.lock().await.stop();
    *state.mixer.lock().await = finch_audio::AudioMixer::new();
    state.stop_screen_share();
    state.stop_watching().await;
}

/// Second half of ending a session on this device: closes QUIC with `reason` and clears the signed-in
/// state. The server is not told and the stored token is not touched.
async fn close_session_locally(state: &AppState, reason: &[u8]) {
    if let Some(conn) = state.quic_connection.write().await.take() {
        conn.close(0u32.into(), reason);
    }
    *state.control_tx.write().await = None;
    *state.auth_token.write().await = None;
    *state.api.write().await = None;
    *state.current_user.write().await = None;
    *state.current_voice_channel.write().await = None;
}

/// Ends the session on this device only (no server revoke, the stored token stays). Used for a
/// successful automatic resume that lost the race to a manual sign-in, so no hidden session survives.
pub async fn discard_local_session(state: &AppState) {
    let _transition = state.voice_transition.lock().await;
    stop_session_activity(state).await;
    state.signed_in_key.write().await.take();
    close_session_locally(state, b"discarded").await;
}

#[tauri::command]
pub async fn discard_session(state: State<'_, AppState>) -> Result<(), String> {
    discard_local_session(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let _transition = state.voice_transition.lock().await;
    stop_session_activity(&state).await;
    forget_stored_token(&state).await;

    let api = state.api.read().await.clone();
    let revoke_error = match api {
        Some(api) => api.logout().await.err(),
        None => None,
    };

    close_session_locally(&state, b"logout").await;

    info!("Logged out successfully");
    match revoke_error {
        Some(error) => Err(format!("Logged out locally, but could not revoke server token: {error}")),
        None => Ok(()),
    }
}

/// "Retry now": stop waiting out the reconnect backoff.
#[tauri::command]
pub fn reconnect_now(state: State<'_, AppState>) {
    state.reconnect_wake.notify_one();
}

#[tauri::command]
pub async fn get_current_user(state: State<'_, AppState>) -> Result<Option<UserDto>, String> {
    Ok(state.current_user.read().await.clone())
}

#[tauri::command]
pub async fn trust_fingerprint(
    hostname: String,
    fingerprint: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.saved_fingerprints.insert(hostname, fingerprint);
    *state.pending_fingerprint.write().await = None;
    if let Some(path) = &*state.known_servers_path.read().await {
        finch_client_core::tofu::save_fingerprints(path, &state.saved_fingerprints)
            .map_err(|e| format!("Failed to save server fingerprint: {}", e))?;
    }
    Ok(())
}

/// Forgets the pinned certificate of `hostname` so the next sign-in asks again (after the user
/// confirmed a legitimate certificate change in the warning dialog).
#[tauri::command]
pub async fn forget_server_fingerprint(hostname: String, state: State<'_, AppState>) -> Result<(), String> {
    state.saved_fingerprints.remove(&hostname);
    *state.pending_fingerprint.write().await = None;
    if let Some(path) = &*state.known_servers_path.read().await {
        finch_client_core::tofu::save_fingerprints(path, &state.saved_fingerprints)
            .map_err(|e| format!("Failed to save server fingerprints: {}", e))?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ServerInfo {
    server_url: Option<String>,
    hostname: Option<String>,
    fingerprint: Option<String>,
}

/// The server we are signed in to and the certificate fingerprint we trust for it (About page).
#[tauri::command]
pub async fn server_info(state: State<'_, AppState>) -> Result<ServerInfo, String> {
    let server_url = state.server_url.read().await.clone();
    let hostname = server_url.as_deref().and_then(|url| {
        let rest = url.split("://").nth(1).unwrap_or(url);
        let host = rest.split('/').next().unwrap_or(rest);
        let host = host.rsplit_once(':').map_or(host, |(name, port)| if port.chars().all(|c| c.is_ascii_digit()) { name } else { host });
        (!host.is_empty()).then(|| host.to_string())
    });
    let fingerprint = hostname.as_ref().and_then(|name| state.saved_fingerprints.get(name).map(|entry| entry.value().clone()));
    Ok(ServerInfo { server_url, hostname, fingerprint })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{MemoryStore, TokenStore};
    use finch_testbed::client::TEST_PASSWORD;
    use finch_testbed::server::TestServer;
    use std::sync::Arc;

    fn no_emit(_: &str, _: serde_json::Value) {}

    fn state_with(store: Arc<MemoryStore>, server: &TestServer) -> AppState {
        let mut state = AppState::new();
        state.token_store = store;
        state.saved_fingerprints.insert("127.0.0.1".into(), server.certificate_fingerprint.clone());
        state
    }

    /// Register + remember, up to (not including) the QUIC connection.
    async fn sign_in(state: &AppState, server: &TestServer, name: &str, remember: bool) -> anyhow::Result<AuthResponse> {
        let api = trusted_api_with(&server.http_url, state, &no_emit).await.map_err(anyhow::Error::msg)?;
        let auth = api.register(name, TEST_PASSWORD, name).await?;
        remember_sign_in(state, &server.http_url, name, &auth.token, remember).await;
        apply_sign_in(api, &auth, &server.http_url, state).await;
        Ok(auth)
    }

    #[tokio::test]
    async fn resume_returns_none_when_store_is_empty() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let state = state_with(Arc::new(MemoryStore::default()), &server);
        let resumed = refresh_stored_session(&server.http_url, "nobody", &state, &no_emit).await;
        assert!(matches!(resumed, Ok(None)));
        Ok(())
    }

    #[tokio::test]
    async fn resume_restores_session_from_stored_token() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let first = state_with(store.clone(), &server);
        let auth = sign_in(&first, &server, "alice", true).await?;
        assert!(first.signed_in_key.read().await.is_some());

        // "Restart": a fresh state sharing only the token store and the pinned fingerprints.
        let second = state_with(store.clone(), &server);
        let (api, resumed) = refresh_stored_session(&server.http_url, "alice", &second, &no_emit)
            .await
            .map_err(anyhow::Error::msg)?
            .ok_or_else(|| anyhow::anyhow!("stored token must resume"))?;
        assert_eq!(resumed.user.id, auth.user.id);
        assert!(api.server_state().await.is_ok(), "the refreshed client is authorised");
        let key = credential_key(&server.http_url, "alice");
        assert_eq!(store.load(&key), Some(resumed.token.clone()));
        assert_eq!(second.signed_in_key.read().await.as_deref(), Some(key.as_str()));
        Ok(())
    }

    #[tokio::test]
    async fn resume_deletes_token_rejected_by_server() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let first = state_with(store.clone(), &server);
        sign_in(&first, &server, "bob", true).await?;
        // The testbed exposes no database handle; revoking the session via logout has the same effect.
        let api = first.api.read().await.clone().ok_or_else(|| anyhow::anyhow!("no api"))?;
        api.logout().await?;

        let second = state_with(store.clone(), &server);
        let resumed = refresh_stored_session(&server.http_url, "bob", &second, &no_emit).await;
        assert!(matches!(resumed, Ok(None)));
        assert_eq!(store.load(&credential_key(&server.http_url, "bob")), None);
        Ok(())
    }

    #[tokio::test]
    async fn resume_keeps_token_when_server_is_unreachable() -> anyhow::Result<()> {
        let mut server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let state = state_with(store.clone(), &server);
        sign_in(&state, &server, "carol", true).await?;
        server.stop().await;
        let resumed = refresh_stored_session(&server.http_url, "carol", &state, &no_emit).await;
        assert!(resumed.is_err());
        assert!(store.load(&credential_key(&server.http_url, "carol")).is_some());
        Ok(())
    }

    #[tokio::test]
    async fn unticked_stay_signed_in_forgets_an_older_stored_token() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let state = state_with(store.clone(), &server);
        sign_in(&state, &server, "gina", true).await?;
        let key = credential_key(&server.http_url, "gina");
        assert!(store.load(&key).is_some());
        // A later sign-in with "Stay signed in" unticked must not leave the older token for the next start.
        remember_sign_in(&state, &server.http_url, "gina", "fresh-token", false).await;
        assert_eq!(store.load(&key), None);
        assert!(state.signed_in_key.read().await.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn not_remembering_stores_nothing() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let state = state_with(store.clone(), &server);
        sign_in(&state, &server, "dave", false).await?;
        assert_eq!(store.load(&credential_key(&server.http_url, "dave")), None);
        assert!(state.signed_in_key.read().await.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn logout_forgets_the_stored_token() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let state = state_with(store.clone(), &server);
        sign_in(&state, &server, "erin", true).await?;
        let key = credential_key(&server.http_url, "erin");
        assert!(store.load(&key).is_some());
        forget_stored_token(&state).await;
        assert_eq!(store.load(&key), None);
        assert!(state.signed_in_key.read().await.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn discarding_a_session_clears_the_local_state_and_keeps_the_stored_token() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let state = state_with(store.clone(), &server);
        sign_in(&state, &server, "hank", true).await?;
        let key = credential_key(&server.http_url, "hank");
        let stored = store.load(&key);
        assert!(stored.is_some());
        let generation = state.session_generation.load(Ordering::SeqCst);

        discard_local_session(&state).await;

        assert!(state.api.read().await.is_none());
        assert!(state.current_user.read().await.is_none());
        assert!(state.auth_token.read().await.is_none());
        assert!(state.signed_in_key.read().await.is_none());
        assert_ne!(state.session_generation.load(Ordering::SeqCst), generation, "discard must stop the refresher");
        assert_eq!(store.load(&key), stored, "discard must not delete the stored token");
        Ok(())
    }

    #[tokio::test]
    async fn refresh_tick_updates_token_everywhere() -> anyhow::Result<()> {
        let server = TestServer::start().await?;
        let store = Arc::new(MemoryStore::default());
        let state = state_with(store.clone(), &server);
        sign_in(&state, &server, "frank", true).await?;
        crate::connection::refresh_token_once(&state, 0).await.map_err(anyhow::Error::msg)?;
        let fresh = state.auth_token.read().await.clone();
        assert!(fresh.is_some());
        assert_eq!(store.load(&credential_key(&server.http_url, "frank")), fresh);
        Ok(())
    }
}
