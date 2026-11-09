//! Join sounds: a short WAV a person picks to be played to a voice room when they join.

use std::path::PathBuf;

use axum::extract::{Multipart, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use finch_protocol::messages::ServerMessage;
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;

/// Largest upload accepted.
pub const MAX_JOIN_SOUND_BYTES: usize = 512 * 1024;
/// Up to this much of an oversized upload is read and thrown away so the client can receive the 413.
const DRAIN_CAP: usize = 1024 * 1024;
/// Longest sound allowed, with a little tolerance for encoder rounding.
pub const MAX_JOIN_SOUND_MS: u32 = 2050;

/// Duration of a PCM 16-bit WAV in milliseconds; refuses anything else. Never panics on junk.
pub fn wav_duration_ms(bytes: &[u8]) -> Result<u32, String> {
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
            let end = body.checked_add(16).ok_or("The WAV file is damaged")?;
            if size < 16 || end > bytes.len() {
                return Err("The WAV file is damaged".to_string());
            }
            let b = &bytes[body..end];
            format = Some((u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]), u32::from_le_bytes([b[4], b[5], b[6], b[7]]), u16::from_le_bytes([b[14], b[15]])));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = format.ok_or("The WAV file is damaged")?;
            if tag != 1 || bits != 16 {
                return Err("Use a 16-bit PCM WAV file".to_string());
            }
            if !(1..=2).contains(&channels) || !(8000..=48000).contains(&rate) {
                return Err("Use a mono or stereo WAV file of 8000-48000 Hz".to_string());
            }
            // A data chunk may claim more than the file holds; count only what is there.
            let available = bytes.len() - body;
            let data_len = size.min(available) as u64;
            let frame = u64::from(channels) * 2;
            let ms = data_len / frame * 1000 / u64::from(rate);
            return u32::try_from(ms).map_err(|_| "The sound is too long".to_string());
        }
        // Chunks are padded to an even size.
        let next = size.checked_add(size & 1).and_then(|s| body.checked_add(s));
        match next {
            Some(n) => pos = n,
            None => break,
        }
    }
    Err("The WAV file is damaged".to_string())
}

fn legacy_sound_file(state: &ArcSharedState, user_id: &str) -> PathBuf {
    PathBuf::from(&state.config.storage.upload_dir).join("join_sounds").join(format!("{user_id}.wav"))
}

/// POST /api/auth/join-sound (multipart field `file`).
pub async fn upload_handler(auth: AuthUser, State(state): State<ArcSharedState>, mut multipart: Multipart) -> Result<StatusCode, ServerError> {
    state.avatar_hits.hit(&format!("join-sound:{}", auth.user_id), std::time::Instant::now(), 20, std::time::Duration::from_secs(3600)).map_err(ServerError::TooManyRequests)?;
    let mut data = Vec::new();
    let mut received = 0usize;
    if let Some(mut field) = multipart.next_field().await.map_err(|e| ServerError::BadRequest(e.to_string()))? {
        while let Some(chunk) = field.chunk().await.map_err(|e| ServerError::BadRequest(e.to_string()))? {
            received += chunk.len();
            if received <= MAX_JOIN_SOUND_BYTES {
                data.extend_from_slice(&chunk);
            } else if received > DRAIN_CAP {
                break;
            }
        }
        if received > MAX_JOIN_SOUND_BYTES {
            return Err(ServerError::FileUpload("The sound must be smaller than 512 KB".into()));
        }
    } else {
        return Err(ServerError::BadRequest("No sound provided".into()));
    }
    let duration = wav_duration_ms(&data).map_err(ServerError::BadRequest)?;
    if duration > MAX_JOIN_SOUND_MS {
        return Err(ServerError::BadRequest("The sound is longer than 2 seconds".into()));
    }

    let user = auth.user_id.to_string();
    sqlx::query("UPDATE users SET join_sound_version = random(), join_sound_data = ? WHERE id = ?")
        .bind(data)
        .bind(&user)
        .execute(&state.pool)
        .await?;
    let _ = tokio::fs::remove_file(legacy_sound_file(&state, &user)).await;
    state.sessions.broadcast_all(ServerMessage::JoinSoundChanged { user_id: auth.user_id }).await;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/auth/join-sound
pub async fn delete_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<StatusCode, ServerError> {
    let user = auth.user_id.to_string();
    let deleted = sqlx::query("UPDATE users SET join_sound_version = NULL, join_sound_data = NULL WHERE id = ? AND join_sound_version IS NOT NULL")
        .bind(&user)
        .execute(&state.pool)
        .await?;
    let _ = tokio::fs::remove_file(legacy_sound_file(&state, &user)).await;
    if deleted.rows_affected() > 0 {
        state.sessions.broadcast_all(ServerMessage::JoinSoundChanged { user_id: auth.user_id }).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/users/{id}/join-sound
pub async fn download_handler(_auth: AuthUser, State(state): State<ArcSharedState>, Path(id): Path<Uuid>, headers: HeaderMap) -> Result<Response, ServerError> {
    let user = id.to_string();
    let (version, data): (i64, Option<Vec<u8>>) = sqlx::query_as("SELECT join_sound_version, join_sound_data FROM users WHERE id = ? AND join_sound_version IS NOT NULL")
        .bind(&user)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ServerError::NotFound("No join sound".into()))?;
    let data = match data {
        Some(data) => data,
        None => tokio::fs::read(legacy_sound_file(&state, &user))
            .await
            .map_err(|_| ServerError::NotFound("No join sound".into()))?,
    };
    let etag = format!("\"{version}\"");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, frames: u32, tag: u16, bits: u16) -> Vec<u8> {
        let data_len = frames * u32::from(channels) * u32::from(bits / 8);
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
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
    fn duration_of_mono_and_stereo() {
        assert_eq!(wav_duration_ms(&wav(24000, 1, 24000, 1, 16)), Ok(1000));
        assert_eq!(wav_duration_ms(&wav(48000, 2, 96000, 1, 16)), Ok(2000));
    }

    #[test]
    fn other_formats_are_refused() {
        assert!(wav_duration_ms(&wav(24000, 1, 100, 3, 16)).is_err());
        assert!(wav_duration_ms(&wav(24000, 1, 100, 1, 8)).is_err());
        assert!(wav_duration_ms(&wav(24000, 3, 100, 1, 16)).is_err());
        assert!(wav_duration_ms(&wav(4000, 1, 100, 1, 16)).is_err());
    }

    #[test]
    fn junk_and_truncation_never_panic() {
        assert!(wav_duration_ms(b"").is_err());
        assert!(wav_duration_ms(b"hello world, this is not a wav").is_err());
        let good = wav(24000, 1, 2400, 1, 16);
        for cut in 0..good.len().min(60) {
            let _ = wav_duration_ms(&good[..cut]);
        }
        let mut huge = good.clone();
        huge[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(wav_duration_ms(&huge), Ok(100)); // only the bytes present count
        let mut bad_chunk = b"RIFF\0\0\0\0WAVEjunk".to_vec();
        bad_chunk.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(wav_duration_ms(&bad_chunk).is_err());
    }
}
