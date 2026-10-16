//! Shared authoritative snapshot for QUIC authentication and frontend reloads.
use std::collections::HashSet;

use axum::{extract::State, Json};
use finch_protocol::models::{Channel, ChannelType, ServerState, User, UserRole, UserStatus, VoiceState};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;
use crate::session::SessionManager;

pub async fn state_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<Json<ServerState>, ServerError> {
    Ok(Json(server_state(&state.pool, &state.sessions, Some(auth.user_id)).await?))
}

/// `for_user` adds the unread counts of that user.
pub async fn server_state(pool: &SqlitePool, sessions: &SessionManager, for_user: Option<Uuid>) -> Result<ServerState, ServerError> {
    let rows = sqlx::query_as::<_, (String, i64, String, String, i64, Option<String>, i64)>(
        "SELECT id, short_id, name, channel_type, position, topic, bitrate_kbps FROM channels ORDER BY position, id",
    ).fetch_all(pool).await?;
    let mut channels = Vec::with_capacity(rows.len());
    for (id, short_id, name, kind, position, topic, bitrate) in rows {
        if let Ok(id) = Uuid::parse_str(&id) {
            channels.push(Channel { id, short_id: short_id as u32, name,
                channel_type: if kind == "voice" { ChannelType::Voice } else { ChannelType::Text },
                position: position as i32, topic, bitrate_kbps: bitrate as u32 });
        }
    }
    // Release DashMap guards before touching the database.
    let peers: Vec<_> = sessions.sessions.iter().map(|session| (
        session.user_id, session.peer_id, session.username.clone(),
        session.current_voice_channel, session.muted, session.deafened, session.sharing_screen,
    )).collect();
    let online_ids: HashSet<_> = peers.iter().map(|peer| peer.0).collect();
    let mut online_users = Vec::with_capacity(online_ids.len());
    if !online_ids.is_empty() {
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT id, username, display_name, avatar_version, role, status_pref, status_text FROM users WHERE id IN (",
        );
        let mut ids = query.separated(", ");
        for id in &online_ids { ids.push_bind(id.to_string()); }
        ids.push_unseparated(") ORDER BY username");
        let profiles = query.build_query_as::<(String, String, String, Option<i64>, String, String, Option<String>)>().fetch_all(pool).await?;
        for (id, username, display_name, avatar_version, role, status_pref, status_text) in profiles {
            if let Ok(id) = Uuid::parse_str(&id) {
                let avatar_url = crate::api::avatars::avatar_url(&id.to_string(), avatar_version);
                online_users.push(User { id, username, display_name, avatar_url,
                    status: status_from_pref(&status_pref), status_text,
                    role: match role.as_str() { "admin" => UserRole::Admin, "moderator" => UserRole::Moderator, _ => UserRole::Member } });
            }
        }
    }
    let voice_states = peers.into_iter().filter_map(|(user_id, peer_id, username, short_id, muted, deafened, sharing_screen)| {
        let channel = channels.iter().find(|channel| Some(channel.short_id) == short_id)?;
        Some(VoiceState { user_id, peer_id, username, channel_id: channel.id, muted, deafened, sharing_screen })
    }).collect();
    let unread = match for_user {
        Some(user_id) => crate::chat::unread_for(pool, user_id).await?,
        None => Vec::new(),
    };
    Ok(ServerState { channels, online_users, voice_states, unread })
}

/// The status stored for a user (`online`, `idle`, `dnd`); anything else counts as online.
pub fn status_from_pref(pref: &str) -> UserStatus {
    match pref {
        "idle" => UserStatus::Idle,
        "dnd" => UserStatus::DoNotDisturb,
        _ => UserStatus::Online,
    }
}

/// How a status is stored.
pub fn pref_of_status(status: UserStatus) -> &'static str {
    match status {
        UserStatus::Idle => "idle",
        UserStatus::DoNotDisturb => "dnd",
        _ => "online",
    }
}

/// Cleaned status text: control characters removed, spaces collapsed, at most 80 characters; empty becomes `None`.
pub fn clean_status_text(text: Option<&str>) -> Option<String> {
    let cleaned: String = text?.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = collapsed.chars().take(80).collect();
    let cut = cut.trim().to_string();
    (!cut.is_empty()).then_some(cut)
}
