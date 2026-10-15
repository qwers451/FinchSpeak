//! Chat messages history REST endpoints.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use finch_protocol::models::{AttachmentInfo, HistoryMessage};
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

    for row in rows {
        let (id_str, author_id_str, author_name, content, _reply_to_id, edited_at, timestamp, client_msg_id) = row;

        let msg_id = Uuid::parse_str(&id_str).unwrap_or_default();
        let author_id = Uuid::parse_str(&author_id_str).unwrap_or_default();
        let attachments = attachments_by_message.remove(&id_str).unwrap_or_default();

        messages.push(HistoryMessage {
            id: msg_id,
            author_id,
            author_name,
            content,
            attachments,
            timestamp,
            edited_at,
            client_msg_id: client_msg_id.and_then(|id| Uuid::parse_str(&id).ok()),
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
