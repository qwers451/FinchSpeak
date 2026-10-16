//! Moderation: member list, roles, kick, ban. Admins only.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::api::invites::now_secs;
use crate::error::ServerError;

#[derive(Debug, Serialize)]
pub struct AdminUserDto {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub banned: bool,
    pub online: bool,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SetRoleRequest {
    pub role: String,
}

fn require_admin(auth: &AuthUser) -> Result<(), ServerError> {
    if auth.role == "admin" {
        Ok(())
    } else {
        Err(ServerError::Forbidden)
    }
}

/// (role, banned) of a user, or 404.
async fn target_account(state: &ArcSharedState, id: &str) -> Result<(String, bool), ServerError> {
    sqlx::query_as::<_, (String, Option<i64>)>("SELECT role, banned_at FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .map(|(role, banned_at)| (role, banned_at.is_some()))
        .ok_or_else(|| ServerError::NotFound("No such user".into()))
}

/// Revoke every token of the user and close their live connections.
async fn disconnect_user(state: &ArcSharedState, user_id: Uuid, reason: &'static [u8]) -> Result<(), ServerError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(user_id.to_string()).execute(&state.pool).await?;
    let connections: Vec<_> = state
        .sessions
        .sessions
        .iter()
        .filter(|session| session.user_id == user_id)
        .map(|session| session.connection.clone())
        .collect();
    for connection in connections {
        connection.close(0u32.into(), reason);
    }
    Ok(())
}

/// GET /api/admin/users
pub async fn list_users_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
) -> Result<Json<Vec<AdminUserDto>>, ServerError> {
    require_admin(&auth)?;
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<i64>, Option<i64>)>(
        "SELECT id, username, display_name, role, banned_at, last_seen FROM users ORDER BY username",
    )
    .fetch_all(&state.pool)
    .await?;
    let online: std::collections::HashSet<Uuid> = state.sessions.sessions.iter().map(|s| s.user_id).collect();
    let users = rows
        .into_iter()
        .map(|(id, username, display_name, role, banned_at, last_seen)| AdminUserDto {
            last_seen,
            online: Uuid::parse_str(&id).is_ok_and(|uuid| online.contains(&uuid)),
            id,
            username,
            display_name,
            role,
            banned: banned_at.is_some(),
        })
        .collect();
    Ok(Json(users))
}

/// PUT /api/admin/users/{id}/role
pub async fn set_role_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
    Json(request): Json<SetRoleRequest>,
) -> Result<StatusCode, ServerError> {
    require_admin(&auth)?;
    if request.role != "admin" && request.role != "member" {
        return Err(ServerError::BadRequest("role must be \"admin\" or \"member\"".into()));
    }
    let (current, _) = target_account(&state, &id.to_string()).await?;
    if current == "admin" && request.role == "member" {
        // Never leave the server without an admin (this also stops an admin locking themselves out).
        let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin' AND banned_at IS NULL")
            .fetch_one(&state.pool)
            .await?;
        if admins <= 1 {
            return Err(ServerError::BadRequest("The server needs at least one admin".into()));
        }
    }
    sqlx::query("UPDATE users SET role = ? WHERE id = ?").bind(&request.role).bind(id.to_string()).execute(&state.pool).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/users/{id}/kick: disconnect and make them log in again.
pub async fn kick_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_admin(&auth)?;
    if id == auth.user_id {
        return Err(ServerError::BadRequest("You cannot kick yourself".into()));
    }
    target_account(&state, &id.to_string()).await?;
    disconnect_user(&state, id, b"kicked").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/users/{id}/ban
pub async fn ban_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_admin(&auth)?;
    if id == auth.user_id {
        return Err(ServerError::BadRequest("You cannot ban yourself".into()));
    }
    let (role, _) = target_account(&state, &id.to_string()).await?;
    if role == "admin" {
        return Err(ServerError::BadRequest("Make them a member before banning an admin".into()));
    }
    sqlx::query("UPDATE users SET banned_at = ? WHERE id = ?").bind(now_secs()).bind(id.to_string()).execute(&state.pool).await?;
    disconnect_user(&state, id, b"banned").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/users/{id}/unban
pub async fn unban_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_admin(&auth)?;
    target_account(&state, &id.to_string()).await?;
    sqlx::query("UPDATE users SET banned_at = NULL WHERE id = ?").bind(id.to_string()).execute(&state.pool).await?;
    Ok(StatusCode::NO_CONTENT)
}
