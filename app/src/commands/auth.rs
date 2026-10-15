//! Authentication and session management commands.

use std::net::{SocketAddr, ToSocketAddrs};

use tauri::{AppHandle, State};
use tracing::{error, info};

use crate::connection::start_quic_session;
use crate::state::{AppState, UserDto};

#[derive(serde::Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserDto,
}

fn parse_quic_addr(server_url: &str) -> anyhow::Result<(SocketAddr, String)> {
    let url = reqwest::Url::parse(server_url)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing host in server_url"))?;
    // By default FinchSpeak server runs REST on port 8443 (or 8080) and QUIC on port 4433
    let port = url.port().unwrap_or(8443);
    let quic_port = if port == 8443 || port == 8080 { 4433 } else { port };

    let addr_str = format!("{}:{}", host, quic_port);
    let mut addrs = addr_str.to_socket_addrs()?;
    let addr = addrs
        .next()
        .ok_or_else(|| anyhow::anyhow!("Could not resolve host: {}", host))?;

    Ok((addr, host.to_string()))
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

    let client = state.http_client.clone();
    let login_url = format!("{}/api/auth/login", server_url.trim_end_matches('/'));

    let resp = client
        .post(&login_url)
        .json(&serde_json::json!({
            "username": username,
            "password": password
        }))
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !resp.status().is_success() {
        let err_body = resp
            .text()
            .await
            .unwrap_or_else(|_| "Unknown server error".into());
        return Err(format!("Login failed: {}", err_body));
    }

    let auth_data: AuthResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Store auth info in state
    *state.server_url.write().await = Some(server_url.clone());
    *state.auth_token.write().await = Some(auth_data.token.clone());
    *state.current_user.write().await = Some(auth_data.user.clone());

    // Connect QUIC
    match parse_quic_addr(&server_url) {
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

    let client = state.http_client.clone();
    let reg_url = format!("{}/api/auth/register", server_url.trim_end_matches('/'));

    let resp = client
        .post(&reg_url)
        .json(&serde_json::json!({
            "username": username,
            "display_name": display_name,
            "password": password
        }))
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !resp.status().is_success() {
        let err_body = resp
            .text()
            .await
            .unwrap_or_else(|_| "Unknown server error".into());
        return Err(format!("Registration failed: {}", err_body));
    }

    let auth_data: AuthResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Store auth info in state
    *state.server_url.write().await = Some(server_url.clone());
    *state.auth_token.write().await = Some(auth_data.token.clone());
    *state.current_user.write().await = Some(auth_data.user.clone());

    // Connect QUIC
    if let Ok((quic_addr, hostname)) = parse_quic_addr(&server_url) {
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
    Ok(())
}
