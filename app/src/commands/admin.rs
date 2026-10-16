//! Administration: invitation codes (and, for admins, moderation).

use finch_client_core::api::{AdminUserDto, ApiClient, AuditEntryDto, InviteDto, ServerOverviewDto};
use uuid::Uuid;
use tauri::State;

use crate::state::AppState;

async fn api(state: &State<'_, AppState>) -> Result<ApiClient, String> {
    state.api.read().await.clone().ok_or_else(|| "Not signed in".to_string())
}

#[tauri::command]
pub async fn create_invite(
    max_uses: Option<i64>,
    expires_hours: Option<i64>,
    state: State<'_, AppState>,
) -> Result<InviteDto, String> {
    api(&state).await?.create_invite(max_uses, expires_hours).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_invites(state: State<'_, AppState>) -> Result<Vec<InviteDto>, String> {
    api(&state).await?.list_invites().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn revoke_invite(code: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.revoke_invite(&code).await.map_err(|e| e.to_string())
}

fn parse_user(id: &str) -> Result<Uuid, String> {
    Uuid::parse_str(id).map_err(|e| format!("Invalid user id: {e}"))
}

#[tauri::command]
pub async fn admin_users(state: State<'_, AppState>) -> Result<Vec<AdminUserDto>, String> {
    api(&state).await?.admin_users().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_user_role(user_id: String, role: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.set_user_role(parse_user(&user_id)?, &role).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kick_user(user_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.kick_user(parse_user(&user_id)?).await.map_err(|e| e.to_string())
}

/// Ban for `duration_secs` seconds, or for good when it is absent.
#[tauri::command]
pub async fn ban_user(user_id: String, duration_secs: Option<i64>, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.ban_user_for(parse_user(&user_id)?, duration_secs).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn audit_log(before: Option<i64>, limit: Option<u32>, state: State<'_, AppState>) -> Result<Vec<AuditEntryDto>, String> {
    api(&state).await?.audit_log(before, limit.unwrap_or(50)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn server_overview(state: State<'_, AppState>) -> Result<ServerOverviewDto, String> {
    api(&state).await?.server_overview().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_registration(mode: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.set_registration(&mode).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn backup_now(state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.backup_now().await.map_err(|e| e.to_string())
}

/// `kick` out of the voice room, `mute` or `unmute` in it.
#[tauri::command]
pub async fn moderate_voice(user_id: String, action: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.moderate_voice(parse_user(&user_id)?, &action).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unban_user(user_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.unban_user(parse_user(&user_id)?).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_channel(
    channel_id: String,
    name: Option<String>,
    topic: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    api(&state)
        .await?
        .update_channel(parse_user(&channel_id)?, name.as_deref(), topic.as_deref())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_channel(channel_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.delete_channel(parse_user(&channel_id)?).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reorder_channels(channel_ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let ids = channel_ids.iter().map(|id| parse_user(id)).collect::<Result<Vec<_>, _>>()?;
    api(&state).await?.reorder_channels(&ids).await.map_err(|e| e.to_string())
}
