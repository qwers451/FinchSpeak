//! Authentication and session management commands.

use finch_client_core::api::{resolve_quic_addr, ApiClient, AuthResponse};
use tauri::{AppHandle, State};
use tracing::{error, info};

use crate::connection::start_quic_session;
use crate::state::{AppState, UserDto};

#[tauri::command]
pub async fn login(
    server_url: String,
    username: String,
    password: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<UserDto, String> {
    info!("Logging in to {} as {}", server_url, username);

    let mut api = ApiClient::new(&server_url).map_err(|e| format!("Network request failed: {}", e))?;
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
    match resolve_quic_addr(&server_url) {
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
                return Err(format!("Logged in via HTTP, but QUIC connection failed: {}", e));
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

    let mut api = ApiClient::new(&server_url).map_err(|e| format!("Network request failed: {}", e))?;
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
    if let Ok((quic_addr, hostname)) = resolve_quic_addr(&server_url) {
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
            return Err(format!("Registered via HTTP, but QUIC connection failed: {}", e));
        }
    }

    Ok(auth_data.user)
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), String> {
    state.audio_engine.lock().await.stop();

    if let Some(conn) = state.quic_connection.write().await.take() {
        conn.close(0u32.into(), b"logout");
    }
    *state.control_tx.write().await = None;
    *state.auth_token.write().await = None;
    *state.api.write().await = None;
    *state.current_user.write().await = None;
    *state.current_voice_channel.write().await = None;

    info!("Logged out successfully");
    Ok(())
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
