//! Invitation codes: admins create them, new users spend them when registering.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;

/// Characters without look-alikes (no 0/O, 1/I/L).
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const CODE_LENGTH: usize = 12;
const DEFAULT_MAX_USES: i64 = 1;
const DEFAULT_EXPIRES_HOURS: i64 = 72;
const MAX_USES_LIMIT: i64 = 1000;

#[derive(Debug, Deserialize, Default)]
pub struct CreateInviteRequest {
    pub max_uses: Option<i64>,
    pub expires_hours: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct InviteDto {
    pub code: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub max_uses: i64,
    pub uses: i64,
}

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn require_admin(auth: &AuthUser) -> Result<(), ServerError> {
    if auth.role == "admin" {
        Ok(())
    } else {
        Err(ServerError::Forbidden)
    }
}

fn random_code() -> String {
    let mut bytes = [0u8; CODE_LENGTH];
    rand_core::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| CODE_ALPHABET[usize::from(*b) % CODE_ALPHABET.len()] as char).collect()
}

/// POST /api/invites
pub async fn create_invite_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    payload: Option<Json<CreateInviteRequest>>,
) -> Result<(StatusCode, Json<InviteDto>), ServerError> {
    require_admin(&auth)?;
    let request = payload.map(|Json(request)| request).unwrap_or_default();
    let max_uses = request.max_uses.unwrap_or(DEFAULT_MAX_USES);
    if !(1..=MAX_USES_LIMIT).contains(&max_uses) {
        return Err(ServerError::BadRequest(format!("max_uses must be between 1 and {MAX_USES_LIMIT}")));
    }
    let hours = request.expires_hours.unwrap_or(DEFAULT_EXPIRES_HOURS);
    if !(0..=24 * 365).contains(&hours) {
        return Err(ServerError::BadRequest("expires_hours must be between 0 and 8760".into()));
    }
    let now = now_secs();
    let invite = InviteDto { code: random_code(), created_at: now, expires_at: now + hours * 3600, max_uses, uses: 0 };
    sqlx::query("INSERT INTO invites (code, created_by, created_at, expires_at, max_uses, uses) VALUES (?, ?, ?, ?, ?, 0)")
        .bind(&invite.code)
        .bind(auth.user_id.to_string())
        .bind(invite.created_at)
        .bind(invite.expires_at)
        .bind(invite.max_uses)
        .execute(&state.pool)
        .await?;
    Ok((StatusCode::CREATED, Json(invite)))
}

/// GET /api/invites
pub async fn list_invites_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
) -> Result<Json<Vec<InviteDto>>, ServerError> {
    require_admin(&auth)?;
    let invites = sqlx::query_as::<_, InviteDto>(
        "SELECT code, created_at, expires_at, max_uses, uses FROM invites ORDER BY created_at DESC, code",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(invites))
}

/// DELETE /api/invites/{code}
pub async fn revoke_invite_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(code): Path<String>,
) -> Result<StatusCode, ServerError> {
    require_admin(&auth)?;
    let result = sqlx::query("DELETE FROM invites WHERE code = ?").bind(&code).execute(&state.pool).await?;
    if result.rows_affected() == 0 {
        return Err(ServerError::NotFound("No such invite".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}
