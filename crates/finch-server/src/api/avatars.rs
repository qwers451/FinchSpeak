//! Profile pictures: upload (re-encoded to 128x128 PNG), download with ETag, removal.

use std::io::Cursor;
use std::path::PathBuf;

use axum::extract::{Multipart, Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use finch_protocol::messages::ServerMessage;
use image::{ImageFormat, ImageReader};
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser, UserDto};
use crate::error::ServerError;

/// Largest upload accepted.
pub const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;
/// Up to this much of an oversized upload is read and thrown away so the client can receive the 413.
const DRAIN_CAP: usize = 3 * 1024 * 1024;
/// Size of the stored picture.
pub const AVATAR_SIDE: u32 = 128;
/// Pictures with more pixels than this are refused before decoding (decompression bombs).
const MAX_PIXELS: u64 = 40_000_000;
const MAX_SIDE: u32 = 16_384;

/// The address clients use for a user's picture; changes with every new picture.
pub fn avatar_url(user_id: &str, version: Option<i64>) -> Option<String> {
    version.map(|v| format!("/api/users/{user_id}/avatar?v={v}"))
}

/// Judge by the bytes, decode safely, crop to a square from the centre, scale, encode as PNG.
pub fn process_avatar(data: &[u8]) -> Result<Vec<u8>, String> {
    let reader = ImageReader::new(Cursor::new(data)).with_guessed_format().map_err(|_| "Not a picture".to_string())?;
    let format = reader.format().ok_or("Not a picture")?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP) {
        return Err("Use a PNG, JPEG or WebP picture".to_string());
    }
    let (width, height) = reader.into_dimensions().map_err(|_| "The picture is damaged".to_string())?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("The picture is too large".to_string());
    }
    let image = image::load_from_memory_with_format(data, format).map_err(|_| "The picture is damaged".to_string())?;
    let side = width.min(height);
    let cropped = image.crop_imm((width - side) / 2, (height - side) / 2, side, side);
    let small = cropped.resize_exact(AVATAR_SIDE, AVATAR_SIDE, image::imageops::FilterType::Lanczos3);
    let mut out = Cursor::new(Vec::new());
    small.write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn avatar_file(state: &ArcSharedState, user_id: &str) -> PathBuf {
    PathBuf::from(&state.config.storage.upload_dir).join("avatars").join(format!("{user_id}.png"))
}

async fn user_dto(state: &ArcSharedState, user_id: Uuid) -> Result<UserDto, ServerError> {
    let row = sqlx::query_as::<_, (String, String, String, Option<i64>)>("SELECT username, display_name, role, avatar_version FROM users WHERE id = ?")
        .bind(user_id.to_string())
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ServerError::NotFound("User not found".into()))?;
    Ok(UserDto { id: user_id.to_string(), username: row.0, display_name: row.1, role: row.2, avatar_url: avatar_url(&user_id.to_string(), row.3) })
}

/// POST /api/auth/avatar (multipart field `file`).
pub async fn upload_handler(auth: AuthUser, State(state): State<ArcSharedState>, mut multipart: Multipart) -> Result<Json<UserDto>, ServerError> {
    // Decoding and re-encoding pictures is work: a person may change theirs a few times an hour.
    state.avatar_hits.hit(&auth.user_id.to_string(), std::time::Instant::now(), 10, std::time::Duration::from_secs(3600)).map_err(ServerError::TooManyRequests)?;
    let mut data = Vec::new();
    let mut received = 0usize;
    if let Some(mut field) = multipart.next_field().await.map_err(|e| ServerError::BadRequest(e.to_string()))? {
        while let Some(chunk) = field.chunk().await.map_err(|e| ServerError::BadRequest(e.to_string()))? {
            received += chunk.len();
            if received <= MAX_AVATAR_BYTES {
                data.extend_from_slice(&chunk);
            } else if received > DRAIN_CAP {
                break;
            }
        }
        // Read what the client is still sending before answering, or it sees a reset instead of the 413.
        if received > MAX_AVATAR_BYTES {
            return Err(ServerError::FileUpload("The picture must be smaller than 2 MB".into()));
        }
    } else {
        return Err(ServerError::BadRequest("No picture provided".into()));
    }
    let png = tokio::task::spawn_blocking(move || process_avatar(&data))
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?
        .map_err(ServerError::BadRequest)?;

    let user = auth.user_id.to_string();
    let path = avatar_file(&state, &user);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| ServerError::Internal(e.to_string()))?;
    }
    tokio::fs::write(&path, &png).await.map_err(|e| ServerError::Internal(e.to_string()))?;
    let version = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    sqlx::query("UPDATE users SET avatar_path = ?, avatar_version = ? WHERE id = ?")
        .bind(path.to_string_lossy().to_string())
        .bind(version)
        .bind(&user)
        .execute(&state.pool)
        .await?;
    let dto = user_dto(&state, auth.user_id).await?;
    state.sessions.broadcast_all(ServerMessage::AvatarChanged { user_id: auth.user_id, avatar_url: dto.avatar_url.clone() }).await;
    Ok(Json(dto))
}

/// DELETE /api/auth/avatar
pub async fn delete_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<StatusCode, ServerError> {
    let user = auth.user_id.to_string();
    let had: Option<i64> = sqlx::query_scalar("SELECT avatar_version FROM users WHERE id = ?").bind(&user).fetch_one(&state.pool).await?;
    sqlx::query("UPDATE users SET avatar_path = NULL, avatar_version = NULL WHERE id = ?").bind(&user).execute(&state.pool).await?;
    let _ = tokio::fs::remove_file(avatar_file(&state, &user)).await;
    if had.is_some() {
        state.sessions.broadcast_all(ServerMessage::AvatarChanged { user_id: auth.user_id, avatar_url: None }).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/users/{id}/avatar
pub async fn download_handler(_auth: AuthUser, State(state): State<ArcSharedState>, Path(id): Path<Uuid>, headers: HeaderMap) -> Result<Response, ServerError> {
    let version: Option<i64> = sqlx::query_scalar("SELECT avatar_version FROM users WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    let version = version.ok_or_else(|| ServerError::NotFound("No picture".into()))?;
    let etag = format!("\"{version}\"");
    let mut response_headers = HeaderMap::new();
    response_headers.insert(ETAG, HeaderValue::from_str(&etag).map_err(|e| ServerError::Internal(e.to_string()))?);
    response_headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));
    if headers.get(IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Ok((StatusCode::NOT_MODIFIED, response_headers).into_response());
    }
    let data = tokio::fs::read(avatar_file(&state, &id.to_string())).await.map_err(|_| ServerError::NotFound("No picture".into()))?;
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("image/png"));
    response_headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    Ok((response_headers, data).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 1]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image).write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn any_shape_becomes_a_square_128_png() {
        for (w, h) in [(640, 360), (100, 400), (128, 128), (5, 5)] {
            let out = process_avatar(&png(w, h)).unwrap();
            let decoded = image::load_from_memory(&out).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (128, 128));
        }
    }

    #[test]
    fn junk_is_refused() {
        assert!(process_avatar(b"hello").is_err());
        assert!(process_avatar(&[]).is_err());
    }

    #[test]
    fn the_address_changes_with_the_version() {
        assert_eq!(avatar_url("u", None), None);
        assert_ne!(avatar_url("u", Some(1)), avatar_url("u", Some(2)));
    }
}
