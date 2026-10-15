//! Channel management REST endpoints.

use axum::extract::State;
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
