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
            "SELECT id, username, display_name, avatar_path, role FROM users WHERE id IN (",
        );
        let mut ids = query.separated(", ");
        for id in &online_ids { ids.push_bind(id.to_string()); }
        ids.push_unseparated(") ORDER BY username");
        let profiles = query.build_query_as::<(String, String, String, Option<String>, String)>().fetch_all(pool).await?;
        for (id, username, display_name, avatar_url, role) in profiles {
            if let Ok(id) = Uuid::parse_str(&id) {
                online_users.push(User { id, username, display_name, avatar_url,
                    status: UserStatus::Online,
                    role: if role == "admin" { UserRole::Admin } else { UserRole::Member } });
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
