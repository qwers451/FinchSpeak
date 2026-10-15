//! Channel management REST endpoints.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use finch_protocol::messages::ServerMessage;
use finch_protocol::models::{Channel, ChannelType};

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelDto {
    pub id: String,
    pub short_id: u32,
    pub name: String,
    pub channel_type: String,
    pub position: i32,
    pub topic: Option<String>,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub channel_type: String, // "text" or "voice"
    pub topic: Option<String>,
    pub bitrate_kbps: Option<u32>,
}

/// Handler for GET /api/channels
pub async fn list_channels_handler(
    _auth: AuthUser,
    State(state): State<ArcSharedState>,
) -> Result<impl IntoResponse, ServerError> {
    let rows = sqlx::query_as::<_, (String, i64, String, String, i64, Option<String>, i64)>(
        "SELECT id, short_id, name, channel_type, position, topic, bitrate_kbps
         FROM channels ORDER BY position ASC, created_at ASC"
    )
    .fetch_all(&state.pool)
    .await?;

    let channels: Vec<ChannelDto> = rows
        .into_iter()
        .map(|(id, short_id, name, channel_type, position, topic, bitrate_kbps)| {
            ChannelDto {
                id,
                short_id: short_id as u32,
                name,
                channel_type,
                position: position as i32,
                topic,
                bitrate_kbps: bitrate_kbps as u32,
            }
        })
        .collect();

    Ok(Json(channels))
}

/// Handler for POST /api/channels
pub async fn create_channel_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Json(payload): Json<CreateChannelRequest>,
) -> Result<impl IntoResponse, ServerError> {
    // Check permission (only admin can create channels)
    if auth.role != "admin" {
        return Err(ServerError::Forbidden);
    }

    let name = payload.name.trim().to_lowercase();
    if name.is_empty() || name.len() > 32 {
        return Err(ServerError::BadRequest(
            "Channel name must be between 1 and 32 characters".to_string(),
        ));
    }

    let channel_type = payload.channel_type.to_lowercase();
    if channel_type != "text" && channel_type != "voice" {
        return Err(ServerError::BadRequest(
            "channel_type must be either 'text' or 'voice'".to_string(),
        ));
    }

    let next_short_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(short_id), 0) + 1 FROM channels")
        .fetch_one(&state.pool)
        .await?;

    let next_position: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) + 1 FROM channels")
        .fetch_one(&state.pool)
        .await?;

    let channel_id = Uuid::new_v4().to_string();
    let bitrate = payload.bitrate_kbps.unwrap_or(64);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    sqlx::query(
        "INSERT INTO channels (id, short_id, name, channel_type, position, topic, bitrate_kbps, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&channel_id)
    .bind(next_short_id)
    .bind(&name)
    .bind(&channel_type)
    .bind(next_position)
    .bind(&payload.topic)
    .bind(bitrate as i64)
    .bind(now)
    .execute(&state.pool)
    .await?;

    // Make the channel usable for voice right away and tell every connected client.
    if let Ok(uuid) = Uuid::parse_str(&channel_id) {
        state.sessions.register_channel(uuid, next_short_id as u32);
        let channel = Channel {
            id: uuid,
            short_id: next_short_id as u32,
            name: name.clone(),
            channel_type: if channel_type == "voice" { ChannelType::Voice } else { ChannelType::Text },
            topic: payload.topic.clone(),
            position: next_position as i32,
            bitrate_kbps: bitrate,
        };
        state.sessions.broadcast_all(ServerMessage::ChannelCreated { channel }).await;
    }

    let created = ChannelDto {
        id: channel_id,
        short_id: next_short_id as u32,
        name,
        channel_type,
        position: next_position as i32,
        topic: payload.topic,
        bitrate_kbps: bitrate,
    };

    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: Option<String>,
    pub topic: Option<String>,
    pub bitrate_kbps: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderRequest {
    /// Every channel id, in the wanted order.
    pub channel_ids: Vec<Uuid>,
}

fn require_admin(auth: &AuthUser) -> Result<(), ServerError> {
    if auth.role == "admin" {
        Ok(())
    } else {
        Err(ServerError::Forbidden)
    }
}

/// Load one channel as the protocol model, or 404.
async fn load_channel(state: &ArcSharedState, id: Uuid) -> Result<Channel, ServerError> {
    let row = sqlx::query_as::<_, (i64, String, String, i64, Option<String>, i64)>(
        "SELECT short_id, name, channel_type, position, topic, bitrate_kbps FROM channels WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ServerError::NotFound("No such channel".into()))?;
    Ok(Channel {
        id,
        short_id: row.0 as u32,
        name: row.1,
        channel_type: if row.2 == "voice" { ChannelType::Voice } else { ChannelType::Text },
        topic: row.4,
        position: row.3 as i32,
        bitrate_kbps: row.5 as u32,
    })
}

/// PATCH /api/channels/{id}
pub async fn update_channel_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateChannelRequest>,
) -> Result<Json<ChannelDto>, ServerError> {
    require_admin(&auth)?;
    load_channel(&state, id).await?;
    if let Some(name) = &payload.name {
        let name = name.trim().to_lowercase();
        if name.is_empty() || name.len() > 32 {
            return Err(ServerError::BadRequest("Channel name must be between 1 and 32 characters".into()));
        }
        sqlx::query("UPDATE channels SET name = ? WHERE id = ?").bind(name).bind(id.to_string()).execute(&state.pool).await?;
    }
    if let Some(topic) = &payload.topic {
        sqlx::query("UPDATE channels SET topic = ? WHERE id = ?").bind(topic).bind(id.to_string()).execute(&state.pool).await?;
    }
    if let Some(bitrate) = payload.bitrate_kbps {
        sqlx::query("UPDATE channels SET bitrate_kbps = ? WHERE id = ?").bind(i64::from(bitrate)).bind(id.to_string()).execute(&state.pool).await?;
    }
    let channel = load_channel(&state, id).await?;
    state.sessions.broadcast_all(ServerMessage::ChannelUpdated { channel: channel.clone() }).await;
    Ok(Json(ChannelDto::from(&channel)))
}

/// PUT /api/channels/order
pub async fn reorder_channels_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Json(payload): Json<ReorderRequest>,
) -> Result<StatusCode, ServerError> {
    require_admin(&auth)?;
    let existing: Vec<String> = sqlx::query_scalar("SELECT id FROM channels").fetch_all(&state.pool).await?;
    let mut wanted: Vec<String> = payload.channel_ids.iter().map(Uuid::to_string).collect();
    let mut have = existing.clone();
    wanted.sort();
    have.sort();
    if wanted != have {
        return Err(ServerError::BadRequest("The order must list every channel exactly once".into()));
    }
    let mut changed = Vec::new();
    let mut tx = state.pool.begin().await?;
    for (index, id) in payload.channel_ids.iter().enumerate() {
        let position = index as i64 + 1;
        let result = sqlx::query("UPDATE channels SET position = ? WHERE id = ? AND position != ?")
            .bind(position)
            .bind(id.to_string())
            .bind(position)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() > 0 {
            changed.push(*id);
        }
    }
    tx.commit().await?;
    for id in changed {
        let channel = load_channel(&state, id).await?;
        state.sessions.broadcast_all(ServerMessage::ChannelUpdated { channel }).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/channels/{id}
pub async fn delete_channel_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_admin(&auth)?;
    let channel = load_channel(&state, id).await?;
    if channel.channel_type == ChannelType::Text {
        let texts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE channel_type = 'text'").fetch_one(&state.pool).await?;
        if texts <= 1 {
            return Err(ServerError::BadRequest("The last text channel cannot be deleted".into()));
        }
    }

    // Files of the channel's messages go with it.
    let files: Vec<String> = sqlx::query_scalar(
        "SELECT a.storage_path FROM attachments a JOIN messages m ON m.id = a.message_id WHERE m.channel_id = ?",
    )
    .bind(id.to_string())
    .fetch_all(&state.pool)
    .await?;

    // Send everyone out of the voice room before it disappears.
    for (peer_id, user_id, was_sharing) in state.sessions.close_voice_room(channel.short_id) {
        if was_sharing {
            state.sessions.broadcast_all(ServerMessage::ScreenShareStopped { peer_id }).await;
        }
        state
            .sessions
            .broadcast_all(ServerMessage::UserLeftVoice { user_id, peer_id, channel_id: id })
            .await;
    }

    sqlx::query("DELETE FROM channels WHERE id = ?").bind(id.to_string()).execute(&state.pool).await?;
    for path in files {
        let _ = std::fs::remove_file(path);
    }
    state.sessions.broadcast_all(ServerMessage::ChannelDeleted { channel_id: id }).await;
    Ok(StatusCode::NO_CONTENT)
}

impl From<&Channel> for ChannelDto {
    fn from(channel: &Channel) -> Self {
        Self {
            id: channel.id.to_string(),
            short_id: channel.short_id,
            name: channel.name.clone(),
            channel_type: if channel.channel_type == ChannelType::Voice { "voice".into() } else { "text".into() },
            position: channel.position,
            topic: channel.topic.clone(),
            bitrate_kbps: channel.bitrate_kbps,
        }
    }
}
