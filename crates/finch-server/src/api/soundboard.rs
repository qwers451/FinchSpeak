//! Shared soundboard: a board of short WAV sounds kept in the database. The server never mixes
//! them into voice; it only relays "play sound X" to a room and every client plays it from its cache.

use std::collections::HashSet;

use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use finch_protocol::models::SoundInfo;
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::config::SoundboardConfig;
use crate::error::ServerError;
use crate::permissions::{self, Permission};
use crate::session::SessionManager;

/// Largest uploaded WAV.
pub const MAX_SOUND_BYTES: usize = 256 * 1024;
/// Longest sound on the board.
pub const MAX_SOUND_MS: u64 = 5000;
/// The only format accepted: 16-bit mono PCM at this rate.
pub const SOUND_RATE: u32 = 24_000;
/// Longest name, in characters.
pub const MAX_NAME_CHARS: usize = 32;
/// Longest emoji field, in characters (a ZWJ sequence can be several code points).
const MAX_EMOJI_CHARS: usize = 32;
/// Up to this much of an oversized upload is read and thrown away so the client can receive the 413.
const DRAIN_CAP: usize = 1024 * 1024;

/// The soundboard settings; a server without them configured keeps the default (enabled).
pub fn board_config(sessions: &SessionManager) -> SoundboardConfig {
    sessions.soundboard.get().cloned().unwrap_or_default()
}

/// Whether the soundboard is switched on.
pub fn is_enabled(sessions: &SessionManager) -> bool {
    board_config(sessions).enabled
}

fn require_enabled(state: &ArcSharedState) -> Result<SoundboardConfig, ServerError> {
    let config = board_config(&state.sessions);
    if config.enabled {
        Ok(config)
    } else {
        Err(ServerError::NotFound("Soundboard is unavailable".into()))
    }
}

/// Checks a WAV is 16-bit mono PCM at 24 kHz and at most 5 seconds; returns its duration in ms.
/// Never panics on junk input.
pub fn check_wav(bytes: &[u8]) -> Result<u64, String> {
    const DAMAGED: &str = "The WAV file is damaged";
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Not a WAV file".to_string());
    }
    let mut pos = 12usize;
    let mut format: Option<(u16, u16, u32, u16)> = None; // tag, channels, rate, bits
    while pos.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            let end = body.checked_add(16).ok_or(DAMAGED)?;
            if size < 16 || end > bytes.len() {
                return Err(DAMAGED.to_string());
            }
            let b = &bytes[body..end];
            format = Some((
                u16::from_le_bytes([b[0], b[1]]),
                u16::from_le_bytes([b[2], b[3]]),
                u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                u16::from_le_bytes([b[14], b[15]]),
            ));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = format.ok_or(DAMAGED)?;
            if tag != 1 || channels != 1 || rate != SOUND_RATE || bits != 16 {
                return Err("The sound must be a 16-bit mono WAV at 24 kHz".to_string());
            }
            // A data chunk may claim more than the file holds; count only what is there.
            let available = bytes.len() - body;
            let data_len = size.min(available) as u64;
            let ms = data_len / 2 * 1000 / u64::from(SOUND_RATE);
            if ms > MAX_SOUND_MS {
                return Err("The sound is longer than 5 seconds".to_string());
            }
            return Ok(ms);
        }
        // Chunks are padded to an even size.
        match size.checked_add(size & 1).and_then(|s| body.checked_add(s)) {
            Some(next) => pos = next,
            None => break,
        }
    }
    Err(DAMAGED.to_string())
}

fn parse_id(text: &str) -> Result<Uuid, ServerError> {
    Uuid::parse_str(text).map_err(|e| ServerError::Internal(e.to_string()))
}

/// GET /api/soundboard
pub async fn list_handler(_auth: AuthUser, State(state): State<ArcSharedState>) -> Result<Json<Vec<SoundInfo>>, ServerError> {
    require_enabled(&state)?;
    let rows: Vec<(String, String, Option<String>, String, i64)> =
        sqlx::query_as("SELECT id, name, emoji, uploader_id, duration_ms FROM soundboard_sounds ORDER BY rowid")
            .fetch_all(&state.pool)
            .await?;
    let sounds = rows
        .into_iter()
        .map(|(id, name, emoji, uploader, duration_ms)| {
            Ok(SoundInfo {
                id: parse_id(&id)?,
                name,
                emoji,
                uploader_id: parse_id(&uploader)?,
                duration_ms: duration_ms.max(0) as u64,
            })
        })
        .collect::<Result<Vec<_>, ServerError>>()?;
    Ok(Json(sounds))
}

/// POST /api/soundboard (multipart: `file` WAV, `name`, optional `emoji`)
pub async fn upload_handler(auth: AuthUser, State(state): State<ArcSharedState>, mut multipart: Multipart) -> Result<(StatusCode, Json<SoundInfo>), ServerError> {
    let config = require_enabled(&state)?;
    let mut data = Vec::new();
    let mut received = 0usize;
    let mut name: Option<String> = None;
    let mut emoji: Option<String> = None;
    let mut seen = HashSet::new();
    while let Some(mut field) = multipart.next_field().await.map_err(|e| ServerError::BadRequest(e.to_string()))? {
        let field_name = field.name().unwrap_or("").to_owned();
        if !seen.insert(field_name.clone()) {
            return Err(ServerError::BadRequest("Duplicate soundboard field".into()));
        }
        match field_name.as_str() {
            "file" => {
                while let Some(chunk) = field.chunk().await.map_err(|e| ServerError::BadRequest(e.to_string()))? {
                    received += chunk.len();
                    if received <= MAX_SOUND_BYTES {
                        data.extend_from_slice(&chunk);
                    } else if received > DRAIN_CAP {
                        break;
                    }
                }
            }
            "name" | "emoji" => {
                let mut value = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(|e| ServerError::BadRequest(e.to_string()))? {
                    if value.len().saturating_add(chunk.len()) > 256 {
                        return Err(ServerError::BadRequest("Soundboard text is too long".into()));
                    }
                    value.extend_from_slice(&chunk);
                }
                let text = String::from_utf8(value).map_err(|_| ServerError::BadRequest("Soundboard text must be UTF-8".into()))?;
                if field_name == "name" {
                    name = Some(text);
                } else {
                    emoji = Some(text);
                }
            }
            _ => return Err(ServerError::BadRequest("Unknown soundboard field".into())),
        }
    }
    if received > MAX_SOUND_BYTES {
        return Err(ServerError::FileUpload("The sound must be smaller than 256 KB".into()));
    }
    if data.is_empty() {
        return Err(ServerError::BadRequest("No sound provided".into()));
    }
    let name = name.unwrap_or_default().trim().to_owned();
    let name_chars = name.chars().count();
    if name_chars == 0 || name_chars > MAX_NAME_CHARS {
        return Err(ServerError::BadRequest("The name must be 1 to 32 characters".into()));
    }
    let emoji = emoji.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty());
    if emoji.as_ref().is_some_and(|text| text.chars().count() > MAX_EMOJI_CHARS) {
        return Err(ServerError::BadRequest("The emoji is too long".into()));
    }
    let duration_ms = check_wav(&data).map_err(ServerError::BadRequest)?;

    let sound = SoundInfo { id: Uuid::new_v4(), name, emoji, uploader_id: auth.user_id, duration_ms };
    // Count and insert in one statement so two uploads cannot both take the last free place.
    let inserted = sqlx::query(
        "INSERT INTO soundboard_sounds (id, name, emoji, uploader_id, duration_ms, data, created_at) \
         SELECT ?, ?, ?, ?, ?, ?, ? WHERE (SELECT COUNT(*) FROM soundboard_sounds) < ?",
    )
    .bind(sound.id.to_string())
    .bind(&sound.name)
    .bind(&sound.emoji)
    .bind(sound.uploader_id.to_string())
    .bind(sound.duration_ms as i64)
    .bind(data)
    .bind(crate::api::invites::now_secs())
    .bind(config.max_sounds as i64)
    .execute(&state.pool)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ServerError::BadRequest(format!("The soundboard is full ({} sounds)", config.max_sounds)));
    }
    Ok((StatusCode::CREATED, Json(sound)))
}

/// GET /api/soundboard/{id}/file
pub async fn file_handler(_auth: AuthUser, State(state): State<ArcSharedState>, Path(id): Path<Uuid>, headers: HeaderMap) -> Result<Response, ServerError> {
    require_enabled(&state)?;
    let data: Vec<u8> = sqlx::query_scalar("SELECT data FROM soundboard_sounds WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ServerError::NotFound("Unknown sound".into()))?;
    // A sound never changes under its id, so the id is its ETag.
    let etag = format!("\"{id}\"");
    let mut response_headers = HeaderMap::new();
    response_headers.insert(ETAG, HeaderValue::from_str(&etag).map_err(|e| ServerError::Internal(e.to_string()))?);
    response_headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    if headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Ok((StatusCode::NOT_MODIFIED, response_headers).into_response());
    }
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("audio/wav"));
    response_headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    Ok((response_headers, data).into_response())
}

/// DELETE /api/soundboard/{id}: the uploader, or anyone with moderator rights.
pub async fn delete_handler(auth: AuthUser, State(state): State<ArcSharedState>, Path(id): Path<Uuid>) -> Result<StatusCode, ServerError> {
    require_enabled(&state)?;
    let uploader: String = sqlx::query_scalar("SELECT uploader_id FROM soundboard_sounds WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ServerError::NotFound("Unknown sound".into()))?;
    if uploader != auth.user_id.to_string() && !permissions::allows(&auth.role, Permission::DeleteAnyMessage) {
        return Err(ServerError::Forbidden);
    }
    sqlx::query("DELETE FROM soundboard_sounds WHERE id = ?")
        .bind(id.to_string())
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, bits: u16, frames: u32) -> Vec<u8> {
        let data_len = frames * u32::from(channels) * u32::from(bits / 8);
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
        out.extend_from_slice(&(channels * (bits / 8)).to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(out.len() + data_len as usize, 0);
        out
    }

    #[test]
    fn only_mono_16_bit_24_khz_up_to_five_seconds_is_accepted() {
        assert_eq!(check_wav(&wav(24_000, 1, 16, 24_000)), Ok(1000));
        assert_eq!(check_wav(&wav(24_000, 1, 16, 120_000)), Ok(5000));
        assert!(check_wav(&wav(24_000, 1, 16, 120_024)).is_err());
        assert!(check_wav(&wav(24_000, 2, 16, 100)).is_err());
        assert!(check_wav(&wav(22_050, 1, 16, 100)).is_err());
        assert!(check_wav(&wav(24_000, 1, 8, 100)).is_err());
    }

    #[test]
    fn junk_and_truncation_never_panic() {
        assert!(check_wav(b"").is_err());
        assert!(check_wav(b"not a wav file at all").is_err());
        let good = wav(24_000, 1, 16, 2400);
        for cut in 0..good.len().min(60) {
            let _ = check_wav(&good[..cut]);
        }
        let mut bad_chunk = b"RIFF\0\0\0\0WAVEjunk".to_vec();
        bad_chunk.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(check_wav(&bad_chunk).is_err());
    }
}
