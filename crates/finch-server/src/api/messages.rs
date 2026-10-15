//! Chat messages history REST endpoints.

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
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<i64>, i64)>(
        "SELECT m.id, m.author_id, u.display_name, m.content, m.reply_to_id, m.edited_at, m.created_at
         FROM messages m
         JOIN users u ON m.author_id = u.id
         WHERE m.channel_id = ? AND m.created_at < ?
         ORDER BY m.created_at DESC
         LIMIT ?"
    )
    .bind(&channel_id_str)
    .bind(before)
    .bind(limit + 1) // +1 to check has_more
    .fetch_all(&state.pool)
    .await?;

    let has_more = rows.len() > limit as usize;
    let mut messages: Vec<HistoryMessage> = Vec::with_capacity(rows.len());

    for row in rows.into_iter().take(limit as usize) {
        let (id_str, author_id_str, author_name, content, _reply_to_id, edited_at, timestamp) = row;

        let msg_id = Uuid::parse_str(&id_str).unwrap_or_default();
        let author_id = Uuid::parse_str(&author_id_str).unwrap_or_default();

        // Fetch attachments for this message
        let attach_rows = sqlx::query_as::<_, (String, String, i64, String)>(
            "SELECT id, filename, size_bytes, content_type FROM attachments WHERE message_id = ?"
        )
        .bind(&id_str)
        .fetch_all(&state.pool)
        .await?;

        let attachments: Vec<AttachmentInfo> = attach_rows
            .into_iter()
            .map(|(att_id, filename, size_bytes, content_type)| {
                let id = Uuid::parse_str(&att_id).unwrap_or_default();
                AttachmentInfo {
                    id,
                    url: format!("/api/files/{att_id}"),
                    filename,
                    size_bytes: size_bytes as u64,
                    content_type,
                }
            })
            .collect();

        messages.push(HistoryMessage {
            id: msg_id,
            author_id,
            author_name,
            content,
            attachments,
            timestamp,
            edited_at,
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
