//! Administration: invitation codes (and, for admins, moderation).

use finch_client_core::api::{AdminUserDto, ApiClient, InviteDto};
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

#[tauri::command]
pub async fn ban_user(user_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.ban_user(parse_user(&user_id)?).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unban_user(user_id: String, state: State<'_, AppState>) -> Result<(), String> {
    api(&state).await?.unban_user(parse_user(&user_id)?).await.map_err(|e| e.to_string())
}
