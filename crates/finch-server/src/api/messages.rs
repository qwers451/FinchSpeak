//! Chat messages history REST endpoints.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use finch_protocol::models::{AttachmentInfo, HistoryMessage, ReactionInfo};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub before: Option<i64>,
    /// Cursor: only messages older than this message id (UUID v7, time-ordered).
    pub before_id: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub channel_id: String,
    pub messages: Vec<HistoryMessage>,
    pub has_more: bool,
}

/// Handler for GET /api/channels/:channel_id/messages
pub async fn channel_messages_handler(
    _auth: AuthUser,
    Path(channel_id_str): Path<String>,
    Query(query): Query<HistoryQuery>,
    State(state): State<ArcSharedState>,
) -> Result<impl IntoResponse, ServerError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 100) as i64;
    let before = query.before.unwrap_or(i64::MAX);

    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE id = ?")
        .bind(&channel_id_str)
        .fetch_one(&state.pool)
        .await?;
    if exists == 0 {
        return Err(ServerError::NotFound("No such channel".into()));
    }

    // Fetch messages with author display_name
    let mut rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<i64>, i64, Option<String>)>(
        "SELECT m.id, m.author_id, u.display_name, m.content, m.reply_to_id, m.edited_at, m.created_at, m.client_msg_id
         FROM messages m
         JOIN users u ON m.author_id = u.id
         WHERE m.channel_id = ? AND m.created_at < ?
           AND (? IS NULL OR m.id < ?)
         ORDER BY m.created_at DESC, m.id DESC
         LIMIT ?"
    )
    .bind(&channel_id_str)
    .bind(before)
    .bind(&query.before_id)
    .bind(&query.before_id)
    .bind(limit + 1) // +1 to check has_more
    .fetch_all(&state.pool)
    .await?;

    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let mut messages: Vec<HistoryMessage> = Vec::with_capacity(rows.len());
    let mut attachments_by_message: HashMap<String, Vec<AttachmentInfo>> = HashMap::new();

    if !rows.is_empty() {
        // Load only the selected page, excluding the has_more lookahead row.
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT message_id, id, filename, size_bytes, content_type FROM attachments WHERE message_id IN (",
        );
        let mut ids = query.separated(", ");
        for row in &rows {
            ids.push_bind(&row.0);
        }
        ids.push_unseparated(")");
        // Preserve the insertion order of the previous per-message table scans.
        query.push(" ORDER BY rowid");
        let attach_rows = query
            .build_query_as::<(String, String, String, i64, String)>()
            .fetch_all(&state.pool)
            .await?;

        for (message_id, att_id, filename, size_bytes, content_type) in attach_rows {
            attachments_by_message
                .entry(message_id)
                .or_default()
                .push(AttachmentInfo {
                    id: Uuid::parse_str(&att_id).unwrap_or_default(),
                    url: format!("/api/files/{att_id}"),
                    filename,
                    size_bytes: size_bytes as u64,
                    content_type,
                });
        }
    }

    // Reactions of the selected page, in the order they were added.
    let mut reactions_by_message: HashMap<String, Vec<ReactionInfo>> = HashMap::new();
    if !rows.is_empty() {
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT message_id, emoji, user_id FROM message_reactions WHERE message_id IN (",
        );
        let mut ids = query.separated(", ");
        for row in &rows {
            ids.push_bind(&row.0);
        }
        ids.push_unseparated(") ORDER BY created_at, rowid");
        let reaction_rows = query.build_query_as::<(String, String, String)>().fetch_all(&state.pool).await?;
        for (message_id, emoji, user_id) in reaction_rows {
            let Ok(user_id) = Uuid::parse_str(&user_id) else { continue };
            let entries = reactions_by_message.entry(message_id).or_default();
            match entries.iter_mut().find(|r| r.emoji == emoji) {
                Some(existing) => existing.user_ids.push(user_id),
                None => entries.push(ReactionInfo { emoji, user_ids: vec![user_id] }),
            }
        }
    }

    for row in rows {
        let (id_str, author_id_str, author_name, content, _reply_to_id, edited_at, timestamp, client_msg_id) = row;

        let msg_id = Uuid::parse_str(&id_str).unwrap_or_default();
        let author_id = Uuid::parse_str(&author_id_str).unwrap_or_default();
        let attachments = attachments_by_message.remove(&id_str).unwrap_or_default();
        let reactions = reactions_by_message.remove(&id_str).unwrap_or_default();

        messages.push(HistoryMessage {
            id: msg_id,
            author_id,
            author_name,
            content,
            attachments,
            timestamp,
            edited_at,
            client_msg_id: client_msg_id.and_then(|id| Uuid::parse_str(&id).ok()),
            reactions,
        });
    }

    // Reverse to chronological order (oldest to newest)
    messages.reverse();

    Ok(Json(HistoryResponse {
        channel_id: channel_id_str,
        messages,
        has_more,
    }))
}

#[derive(Debug, Deserialize)]
pub struct EditMessageRequest {
    pub content: String,
}

/// (channel id, author id) of a message, or 404.
async fn message_owner(state: &ArcSharedState, id: Uuid) -> Result<(Uuid, Uuid), ServerError> {
    let row = sqlx::query_as::<_, (String, String)>("SELECT channel_id, author_id FROM messages WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ServerError::NotFound("No such message".into()))?;
    let parse = |text: &str| Uuid::parse_str(text).map_err(|_| ServerError::Internal("corrupt message row".into()));
    Ok((parse(&row.0)?, parse(&row.1)?))
}

/// PATCH /api/messages/{id}: only the author edits.
pub async fn edit_message_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
    Json(request): Json<EditMessageRequest>,
) -> Result<axum::http::StatusCode, ServerError> {
    let (channel_id, author_id) = message_owner(&state, id).await?;
    if author_id != auth.user_id {
        return Err(ServerError::Forbidden);
    }
    let content = request.content.trim().to_string();
    if content.is_empty() || content.chars().count() > crate::quic::MAX_MESSAGE_CHARS {
        return Err(ServerError::BadRequest(format!(
            "Message must be 1..={} characters",
            crate::quic::MAX_MESSAGE_CHARS
        )));
    }
    let edited_at = crate::api::invites::now_secs();
    sqlx::query("UPDATE messages SET content = ?, edited_at = ? WHERE id = ?")
        .bind(&content)
        .bind(edited_at)
        .bind(id.to_string())
        .execute(&state.pool)
        .await?;
    state
        .sessions
        .broadcast_all(finch_protocol::messages::ServerMessage::MessageEdited { message_id: id, channel_id, content, edited_at })
        .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// DELETE /api/messages/{id}: the author or an admin.
pub async fn delete_message_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ServerError> {
    let (channel_id, author_id) = message_owner(&state, id).await?;
    if author_id != auth.user_id && auth.role != "admin" {
        return Err(ServerError::Forbidden);
    }
    let files: Vec<String> = sqlx::query_scalar("SELECT storage_path FROM attachments WHERE message_id = ?")
        .bind(id.to_string())
        .fetch_all(&state.pool)
        .await?;
    sqlx::query("DELETE FROM messages WHERE id = ?").bind(id.to_string()).execute(&state.pool).await?;
    for path in files {
        let _ = std::fs::remove_file(path);
    }
    state
        .sessions
        .broadcast_all(finch_protocol::messages::ServerMessage::MessageDeleted { message_id: id, channel_id })
        .await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

const MAX_EMOJI_CHARS: usize = 16;
const MAX_REACTIONS_PER_MESSAGE: i64 = 20;

fn valid_emoji(emoji: &str) -> bool {
    !emoji.trim().is_empty() && emoji.chars().count() <= MAX_EMOJI_CHARS && !emoji.chars().any(char::is_whitespace)
}

/// PUT /api/messages/{id}/reactions/{emoji}: add your reaction (adding twice changes nothing).
pub async fn add_reaction_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path((id, emoji)): Path<(Uuid, String)>,
) -> Result<axum::http::StatusCode, ServerError> {
    if !valid_emoji(&emoji) {
        return Err(ServerError::BadRequest("Not a valid reaction".into()));
    }
    let (channel_id, _) = message_owner(&state, id).await?;
    let distinct: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT emoji) FROM message_reactions WHERE message_id = ? AND emoji != ?")
        .bind(id.to_string())
        .bind(&emoji)
        .fetch_one(&state.pool)
        .await?;
    if distinct >= MAX_REACTIONS_PER_MESSAGE {
        return Err(ServerError::BadRequest("This message has too many different reactions".into()));
    }
    let inserted = sqlx::query("INSERT OR IGNORE INTO message_reactions (message_id, user_id, emoji, created_at) VALUES (?, ?, ?, ?)")
        .bind(id.to_string())
        .bind(auth.user_id.to_string())
        .bind(&emoji)
        .bind(crate::api::invites::now_secs())
        .execute(&state.pool)
        .await?
        .rows_affected();
    if inserted > 0 {
        state
            .sessions
            .broadcast_all(finch_protocol::messages::ServerMessage::ReactionChanged {
                message_id: id,
                channel_id,
                user_id: auth.user_id,
                emoji,
                added: true,
            })
            .await;
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// DELETE /api/messages/{id}/reactions/{emoji}: remove your reaction (missing is fine).
pub async fn remove_reaction_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path((id, emoji)): Path<(Uuid, String)>,
) -> Result<axum::http::StatusCode, ServerError> {
    let (channel_id, _) = message_owner(&state, id).await?;
    let removed = sqlx::query("DELETE FROM message_reactions WHERE message_id = ? AND user_id = ? AND emoji = ?")
        .bind(id.to_string())
        .bind(auth.user_id.to_string())
        .bind(&emoji)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if removed > 0 {
        state
            .sessions
            .broadcast_all(finch_protocol::messages::ServerMessage::ReactionChanged {
                message_id: id,
                channel_id,
                user_id: auth.user_id,
                emoji,
                added: false,
            })
            .await;
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
