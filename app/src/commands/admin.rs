//! Administration: invitation codes (and, for admins, moderation).

use finch_client_core::api::{ApiClient, InviteDto};
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
