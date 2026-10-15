//! Authentication and session management commands.

use finch_client_core::api::{resolve_quic_addr_with_port, ApiClient, AuthResponse};
use finch_client_core::error::ClientError;
use tauri::{AppHandle, Emitter, State};
use tracing::{error, info};

use crate::connection::start_quic_session;
use crate::state::{AppState, UserDto};

#[derive(serde::Serialize)]
pub struct PendingFingerprint {
    hostname: String,
    fingerprint: String,
}

#[derive(serde::Serialize)]
pub struct SessionSnapshot {
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
        connected, user_id: user.id,
        session_peer_id: state.session_peer_id.load(Ordering::SeqCst),
        current_voice_channel: *state.current_voice_channel.read().await,
        muted: state.is_muted.load(Ordering::SeqCst),
        deafened: state.is_deafened.load(Ordering::SeqCst),
        pending_tofu, initial_state,
    }))
}

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
    let api = ApiClient::with_tofu(server_url, state.saved_fingerprints.clone())
        .map_err(|e| format!("Invalid server address: {e}"))?;
    match api.verify_trust().await {
        Ok(()) => Ok(api),
        Err(ClientError::UntrustedServer { hostname, fingerprint }) => {
            *state.pending_fingerprint.write().await = Some((hostname.clone(), fingerprint.clone()));
            let _ = app_handle.emit(
                "tofu-required",
                serde_json::json!({ "hostname": hostname, "fingerprint": fingerprint }),
            );
            Err(format!(
                "Server certificate of {hostname} is not trusted yet - confirm its fingerprint and sign in again"
            ))
        }
        Err(ClientError::FingerprintMismatch { hostname, .. }) => Err(format!(
            "The certificate of {hostname} changed since you trusted it. Someone may be intercepting              the connection; do not sign in unless the server administrator confirms the change."
        )),
        Err(e) => Err(format!("Cannot reach the server: {e}")),
    }
}

#[tauri::command]
pub async fn login(
    server_url: String,
    username: String,
    password: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<UserDto, String> {
    info!("Logging in to {} as {}", server_url, username);

    let mut api = trusted_api(&server_url, &state, &app_handle).await?;
    let auth_data: AuthResponse = api
        .login(&username, &password)
        .await
        .map_err(|e| format!("Login failed: {}", e))?;
    api.set_token(auth_data.token.clone());
    *state.api.write().await = Some(api);

    // Store auth info in state
    *state.server_url.write().await = Some(server_url.clone());
    *state.auth_token.write().await = Some(auth_data.token.clone());
    *state.current_user.write().await = Some(auth_data.user.clone());

    // Connect QUIC
    match resolve_quic_addr_with_port(&server_url, auth_data.quic_port) {
        Ok((quic_addr, hostname)) => {
            *state.server_quic_addr.write().await = Some(quic_addr);
            if let Err(e) = start_quic_session(
                quic_addr,
                &hostname,
                auth_data.token.clone(),
                (*state).clone(),
                app_handle,
            )
            .await
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

    Ok(auth_data.user)
}

#[tauri::command]
pub async fn register(
    server_url: String,
    username: String,
    display_name: String,
    password: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<UserDto, String> {
    info!("Registering to {} as {}", server_url, username);

    let mut api = trusted_api(&server_url, &state, &app_handle).await?;
    let auth_data: AuthResponse = api
        .register(&username, &password, &display_name)
        .await
        .map_err(|e| format!("Registration failed: {}", e))?;
    api.set_token(auth_data.token.clone());
    *state.api.write().await = Some(api);

    // Store auth info in state
    *state.server_url.write().await = Some(server_url.clone());
    *state.auth_token.write().await = Some(auth_data.token.clone());
    *state.current_user.write().await = Some(auth_data.user.clone());

    // Connect QUIC
    if let Ok((quic_addr, hostname)) = resolve_quic_addr_with_port(&server_url, auth_data.quic_port) {
        *state.server_quic_addr.write().await = Some(quic_addr);
        if let Err(e) = start_quic_session(
            quic_addr,
            &hostname,
            auth_data.token.clone(),
            (*state).clone(),
            app_handle,
        )
        .await
        {
            error!("QUIC connection failed: {}", e);
            return Err(format!("Registered, but the QUIC connection failed: {}", e));
        }
    }

    Ok(auth_data.user)
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    // Stop any reconnect loop before the connection is closed.
    state
        .session_generation
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    *state.last_connect.write().await = None;
    state.audio_engine.lock().await.stop();
    state.stop_screen_share();
    state.stop_watching().await;

    let api = state.api.read().await.clone();
    let revoke_error = match api {
        Some(api) => api.logout().await.err(),
        None => None,
    };

    if let Some(conn) = state.quic_connection.write().await.take() {
        conn.close(0u32.into(), b"logout");
    }
    *state.control_tx.write().await = None;
    *state.auth_token.write().await = None;
    *state.api.write().await = None;
    *state.current_user.write().await = None;
    *state.current_voice_channel.write().await = None;

    info!("Logged out successfully");
    match revoke_error {
        Some(error) => Err(format!("Logged out locally, but could not revoke server token: {error}")),
        None => Ok(()),
    }
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
