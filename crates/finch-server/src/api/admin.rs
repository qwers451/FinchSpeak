//! Moderation: member list, roles, kick, ban, the action log and the server page.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::api::invites::now_secs;
use crate::audit;
use crate::config::RegistrationMode;
use crate::error::ServerError;
use crate::permissions::{may_act_on, require, Permission, ROLES, ROLE_ADMIN};
use crate::settings;

#[derive(Debug, Serialize)]
pub struct AdminUserDto {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub banned: bool,
    /// When a timed ban ends (unix seconds); `None` for a permanent one or no ban.
    pub banned_until: Option<i64>,
    pub online: bool,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SetRoleRequest {
    pub role: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct BanRequest {
    /// Seconds from now; absent or 0 means for good.
    pub duration_secs: Option<i64>,
}

/// Longest timed ban (a year); anything longer is "for good".
const MAX_BAN_SECS: i64 = 365 * 86_400;

/// (role, banned) of a user, or 404.
async fn target_account(state: &ArcSharedState, id: &str) -> Result<(String, bool), ServerError> {
    sqlx::query_as::<_, (String, Option<i64>)>("SELECT role, banned_at FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .map(|(role, banned_at)| (role, banned_at.is_some()))
        .ok_or_else(|| ServerError::NotFound("No such user".into()))
}

async fn name_of(state: &ArcSharedState, id: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id = ?").bind(id).fetch_optional(&state.pool).await.ok().flatten().unwrap_or_else(|| id.to_string())
}

/// Revoke every token of the user and close their live connections.
pub async fn disconnect_user(state: &ArcSharedState, user_id: Uuid, reason: &'static [u8]) -> Result<(), ServerError> {
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
    require(&auth, Permission::ViewMembers)?;
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<i64>, Option<i64>, Option<i64>)>(
        "SELECT id, username, display_name, role, banned_at, banned_until, last_seen FROM users ORDER BY username",
    )
    .fetch_all(&state.pool)
    .await?;
    let online: std::collections::HashSet<Uuid> = state.sessions.sessions.iter().map(|s| s.user_id).collect();
    let now = now_secs();
    let users = rows
        .into_iter()
        .map(|(id, username, display_name, role, banned_at, banned_until, last_seen)| {
            let banned = banned_at.is_some() && banned_until.is_none_or(|until| until > now);
            AdminUserDto {
                online: Uuid::parse_str(&id).is_ok_and(|uuid| online.contains(&uuid)),
                id,
                username,
                display_name,
                role,
                banned,
                banned_until: if banned { banned_until } else { None },
                last_seen,
            }
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
    require(&auth, Permission::ManageMembers)?;
    if !ROLES.contains(&request.role.as_str()) {
        return Err(ServerError::BadRequest("role must be \"admin\", \"moderator\" or \"member\"".into()));
    }
    let (current, _) = target_account(&state, &id.to_string()).await?;
    if current == ROLE_ADMIN && request.role != ROLE_ADMIN {
        // Never leave the server without an admin (this also stops an admin locking themselves out).
        let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin' AND banned_at IS NULL")
            .fetch_one(&state.pool)
            .await?;
        if admins <= 1 {
            return Err(ServerError::BadRequest("The server needs at least one admin".into()));
        }
    }
    sqlx::query("UPDATE users SET role = ? WHERE id = ?").bind(&request.role).bind(id.to_string()).execute(&state.pool).await?;
    audit::record(&state.pool, &auth, "role_changed", Some(&name_of(&state, &id.to_string()).await), Some(&format!("{current} -> {}", request.role))).await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/users/{id}/kick: disconnect and make them log in again.
pub async fn kick_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require(&auth, Permission::KickMember)?;
    if id == auth.user_id {
        return Err(ServerError::BadRequest("You cannot kick yourself".into()));
    }
    let (role, _) = target_account(&state, &id.to_string()).await?;
    if !may_act_on(&auth.role, &role) {
        return Err(ServerError::Forbidden);
    }
    disconnect_user(&state, id, b"kicked").await?;
    audit::record(&state.pool, &auth, "kick", Some(&name_of(&state, &id.to_string()).await), None).await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/users/{id}/ban (optional body: `{ "duration_secs": 3600 }`)
pub async fn ban_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
    request: Option<Json<BanRequest>>,
) -> Result<StatusCode, ServerError> {
    require(&auth, Permission::ManageMembers)?;
    if id == auth.user_id {
        return Err(ServerError::BadRequest("You cannot ban yourself".into()));
    }
    let (role, _) = target_account(&state, &id.to_string()).await?;
    if role == ROLE_ADMIN {
        return Err(ServerError::BadRequest("Make them a member before banning an admin".into()));
    }
    let seconds = request.and_then(|Json(r)| r.duration_secs).filter(|s| *s > 0);
    let now = now_secs();
    let until = seconds.filter(|s| *s < MAX_BAN_SECS).map(|s| now + s);
    sqlx::query("UPDATE users SET banned_at = ?, banned_until = ? WHERE id = ?").bind(now).bind(until).bind(id.to_string()).execute(&state.pool).await?;
    disconnect_user(&state, id, b"banned").await?;
    let details = until.map_or_else(|| "permanent".to_string(), |u| format!("until {u}"));
    audit::record(&state.pool, &auth, "ban", Some(&name_of(&state, &id.to_string()).await), Some(&details)).await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/users/{id}/unban
pub async fn unban_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require(&auth, Permission::ManageMembers)?;
    target_account(&state, &id.to_string()).await?;
    sqlx::query("UPDATE users SET banned_at = NULL, banned_until = NULL WHERE id = ?").bind(id.to_string()).execute(&state.pool).await?;
    audit::record(&state.pool, &auth, "unban", Some(&name_of(&state, &id.to_string()).await), None).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---- action log

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Only entries older than this id.
    pub before: Option<i64>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AuditEntryDto {
    pub id: i64,
    pub at: i64,
    pub actor_name: String,
    pub action: String,
    pub target: Option<String>,
    pub details: Option<String>,
}

/// GET /api/admin/audit?before=&limit=: newest first.
pub async fn audit_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEntryDto>>, ServerError> {
    require(&auth, Permission::ReadAudit)?;
    let limit = i64::from(query.limit.unwrap_or(50).clamp(1, 200));
    let rows = sqlx::query_as::<_, (i64, i64, String, String, Option<String>, Option<String>)>(
        "SELECT id, at, actor_name, action, target, details FROM audit_log WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2",
    )
    .bind(query.before)
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(|(id, at, actor_name, action, target, details)| AuditEntryDto { id, at, actor_name, action, target, details }).collect()))
}

// ---- server page

#[derive(Debug, Serialize)]
pub struct ServerOverview {
    pub version: &'static str,
    pub uptime_secs: u64,
    pub certificate_fingerprint: Option<String>,
    pub quic_port: u16,
    pub http_port: u16,
    pub registration: &'static str,
    pub backups_count: usize,
    pub last_backup_at: Option<i64>,
    pub last_backup_bytes: Option<u64>,
    pub backup_dir: String,
    pub uploads_bytes: u64,
    pub log_dir: String,
}

fn dir_size(path: &std::path::Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| match entry.metadata() {
                    Ok(meta) if meta.is_dir() => dir_size(&entry.path()),
                    Ok(meta) => meta.len(),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

/// GET /api/admin/server
pub async fn overview_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<Json<ServerOverview>, ServerError> {
    require(&auth, Permission::ManageServer)?;
    let backup_dir = std::path::PathBuf::from(&state.config.backup.dir);
    let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)
        .map(|entries| entries.filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().starts_with("finchspeak-")).collect())
        .unwrap_or_default();
    backups.sort_by_key(|entry| entry.file_name());
    let newest = backups.last().and_then(|entry| entry.metadata().ok());
    let last_backup_at = newest
        .as_ref()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let registration = settings::registration_mode(&state.pool, state.config.auth.registration).await;
    Ok(Json(ServerOverview {
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: crate::api::health::uptime_secs(),
        certificate_fingerprint: settings::get(&state.pool, "cert_fingerprint").await,
        quic_port: state.config.server.quic_port,
        http_port: state.config.server.http_port,
        registration: settings::registration_text(registration),
        backups_count: backups.len(),
        last_backup_at,
        last_backup_bytes: newest.map(|meta| meta.len()),
        backup_dir: state.config.backup.dir.clone(),
        uploads_bytes: dir_size(std::path::Path::new(&state.config.storage.upload_dir)),
        log_dir: state.config.logging.dir.clone(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct RegistrationRequest {
    pub mode: String,
}

/// PUT /api/admin/registration `{ "mode": "open" | "invite" }`
pub async fn set_registration_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Json(request): Json<RegistrationRequest>,
) -> Result<StatusCode, ServerError> {
    require(&auth, Permission::ManageServer)?;
    let mode = match request.mode.as_str() {
        "open" => RegistrationMode::Open,
        "invite" => RegistrationMode::Invite,
        _ => return Err(ServerError::BadRequest("mode must be \"open\" or \"invite\"".into())),
    };
    settings::set(&state.pool, "registration", settings::registration_text(mode)).await?;
    audit::record(&state.pool, &auth, "registration_changed", None, Some(settings::registration_text(mode))).await;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/backup: a backup right now.
pub async fn backup_now_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<StatusCode, ServerError> {
    require(&auth, Permission::ManageServer)?;
    crate::backup::create_backup(&state.pool, std::path::Path::new(&state.config.backup.dir), state.config.backup.keep)
        .await
        .map_err(|e| ServerError::Internal(format!("Backup failed: {e}")))?;
    audit::record(&state.pool, &auth, "backup", None, None).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct VoiceModerationRequest {
    /// `kick`, `mute` or `unmute`.
    pub action: String,
}

/// POST /api/admin/users/{id}/voice: send someone out of their voice room, or silence/un-silence them there.
pub async fn moderate_voice_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
    Json(request): Json<VoiceModerationRequest>,
) -> Result<StatusCode, ServerError> {
    use finch_protocol::messages::ServerMessage;
    require(&auth, Permission::ModerateVoice)?;
    if !matches!(request.action.as_str(), "kick" | "mute" | "unmute") {
        return Err(ServerError::BadRequest("action must be \"kick\", \"mute\" or \"unmute\"".into()));
    }
    if id == auth.user_id {
        return Err(ServerError::BadRequest("You cannot moderate yourself".into()));
    }
    let (role, _) = target_account(&state, &id.to_string()).await?;
    if !may_act_on(&auth.role, &role) {
        return Err(ServerError::Forbidden);
    }
    // The session of that person that is in a voice room.
    let in_room = state
        .sessions
        .sessions
        .iter()
        .find(|s| s.user_id == id && s.current_voice_channel.is_some())
        .map(|s| (s.peer_id, s.current_voice_channel.unwrap_or_default(), s.control_tx.clone(), s.sharing_screen));
    let Some((peer_id, short_id, control_tx, was_sharing)) = in_room else {
        return Err(ServerError::BadRequest("They are not in a voice room".into()));
    };
    let channel_id = state.sessions.short_to_channel.get(&short_id).map(|c| *c);
    match request.action.as_str() {
        "kick" => {
            state.sessions.leave_voice(peer_id, short_id);
            if was_sharing && state.sessions.set_sharing(peer_id, false) {
                state.sessions.broadcast_voice(short_id, ServerMessage::ScreenShareStopped { peer_id, reason: finch_protocol::models::ShareEndReason::Left }, Some(peer_id)).await;
            }
            if let Some(channel_id) = channel_id {
                let _ = control_tx.send(ServerMessage::VoiceKicked { channel_id }).await;
                state.sessions.broadcast_all(ServerMessage::UserLeftVoice { user_id: id, peer_id, channel_id }).await;
            }
        }
        _ => {
            let muted = request.action == "mute";
            if let Some(mut session) = state.sessions.sessions.get_mut(&peer_id) {
                session.server_muted = muted;
            }
            state.sessions.broadcast_all(ServerMessage::ServerMuteChanged { peer_id, server_muted: muted }).await;
        }
    }
    audit::record(&state.pool, &auth, &format!("voice_{}", request.action), Some(&name_of(&state, &id.to_string()).await), None).await;
    Ok(StatusCode::NO_CONTENT)
}
