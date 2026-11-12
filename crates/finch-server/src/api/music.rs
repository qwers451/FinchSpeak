//! Authenticated shared library and bounded streaming upload/download endpoints.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Multipart, Path, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use finch_protocol::models::MusicTrack;
use tokio::io::AsyncWriteExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;
use crate::music::{self, MIB, MusicService, UploadGuard};
use crate::permissions::{self, Permission};

fn service(state: &ArcSharedState) -> Result<Arc<MusicService>, ServerError> {
    let music = state
        .sessions
        .music
        .get()
        .cloned()
        .ok_or_else(|| ServerError::NotFound("Music is unavailable".into()))?;
    music.require_enabled()?;
    Ok(music)
}

pub async fn library(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
) -> Result<Json<Vec<MusicTrack>>, ServerError> {
    let _ = auth;
    Ok(Json(service(&state)?.library().await?))
}

pub async fn upload(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<MusicTrack>), ServerError> {
    let music = service(&state)?;
    music
        .upload_hits
        .hit(
            &auth.user_id.to_string(),
            Instant::now(),
            10,
            Duration::from_secs(60),
        )
        .map_err(ServerError::TooManyRequests)?;
    tokio::fs::create_dir_all(&music.dir)
        .await
        .map_err(music::io_error)?;
    let mut guard = UploadGuard::new(music.clone());
    let result = receive_upload(&auth, &music, &mut guard, multipart).await;
    if result.is_err() {
        guard.discard().await;
    }
    result
}

async fn receive_upload(
    auth: &AuthUser,
    music: &Arc<MusicService>,
    guard: &mut UploadGuard,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<MusicTrack>), ServerError> {
    let mut fields = HashMap::new();
    let mut seen = HashSet::new();
    let mut size = 0u64;
    let mut signature = Vec::with_capacity(12);
    let mut has_file = false;
    while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
        let name = field.name().unwrap_or("").to_owned();
        if !seen.insert(name.clone()) {
            return Err(ServerError::BadRequest(
                "Duplicate music upload field".into(),
            ));
        }
        if name == "file" {
            has_file = true;
            music
                .reserve(
                    guard.id,
                    auth.user_id,
                    fields.get("keep").is_some_and(|value| value == "true"),
                    0,
                )
                .await?;
            let mut file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&guard.partial)
                .await
                .map_err(music::io_error)?;
            while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
                size = size.saturating_add(chunk.len() as u64);
                if size > music.config.max_track_mb.saturating_mul(MIB) {
                    return Err(ServerError::FileUpload("Music file is too large".into()));
                }
                music
                    .reserve(
                        guard.id,
                        auth.user_id,
                        fields.get("keep").is_some_and(|value| value == "true"),
                        size,
                    )
                    .await?;
                signature.extend_from_slice(&chunk[..chunk.len().min(12 - signature.len())]);
                file.write_all(&chunk).await.map_err(music::io_error)?;
            }
            file.flush().await.map_err(music::io_error)?;
        } else if matches!(name.as_str(), "title" | "artist" | "duration_ms" | "keep") {
            let mut value = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
                if value.len().saturating_add(chunk.len()) > 2048 {
                    return Err(ServerError::BadRequest("Music metadata is too long".into()));
                }
                value.extend_from_slice(&chunk);
            }
            let value = String::from_utf8(value)
                .map_err(|_| ServerError::BadRequest("Music metadata must be UTF-8".into()))?;
            fields.insert(name, value);
        } else {
            return Err(ServerError::BadRequest("Unknown music upload field".into()));
        }
    }
    if !has_file {
        return Err(ServerError::BadRequest("Music file is required".into()));
    }
    let (title, artist, duration_ms, keep) =
        music::validate_metadata(&fields, music.config.max_track_minutes)?;
    let format = music::detect_format(&signature)?.to_owned();
    // Reclassify a file-first multipart under its final quota before publishing.
    music.reserve(guard.id, auth.user_id, keep, size).await?;
    let track = MusicTrack {
        id: guard.id,
        uploader_id: auth.user_id,
        title,
        artist,
        duration_ms,
        size_bytes: size,
        format,
        temporary: !keep,
        created_at: crate::api::invites::now_secs(),
    };
    let final_path = music.file_path(track.id, &track.format)?;
    let _catalogue = music.catalogue.lock().await;
    guard.final_path = Some(final_path.clone());
    tokio::fs::rename(&guard.partial, &final_path)
        .await
        .map_err(music::io_error)?;
    sqlx::query("INSERT INTO music_tracks (id, uploader_id, title, artist, duration_ms, size_bytes, format, path, temporary, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(track.id.to_string()).bind(track.uploader_id.to_string()).bind(&track.title).bind(&track.artist)
        .bind(track.duration_ms as i64).bind(track.size_bytes as i64).bind(&track.format)
        .bind(final_path.to_string_lossy().as_ref()).bind(track.temporary).bind(track.created_at)
        .execute(&music.pool).await?;
    guard.published = true;
    music.release_reservation(guard.id);
    Ok((StatusCode::CREATED, Json(track)))
}

fn multipart_error(error: axum::extract::multipart::MultipartError) -> ServerError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ServerError::FileUpload("Music file is too large".into())
    } else {
        ServerError::BadRequest("Incomplete or invalid music upload".into())
    }
}

pub async fn download(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
    request: Request,
) -> Result<Response, ServerError> {
    let music = service(&state)?;
    let record = music.record(id).await?;
    let queued_in = record.queued_in;
    let track = record.track()?;
    if track.temporary && !music.can_download_temporary(&state.sessions, auth.user_id, &track, queued_in) {
        return Err(ServerError::NotFound("Music track not found".into()));
    }
    let path = music.file_path(id, &track.format)?;
    let mut response = ServeFile::new(path)
        .try_call(request)
        .await
        .map_err(music::io_error)?
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(music::mime(&track.format)?),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

pub async fn delete(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    let music = service(&state)?;
    let _catalogue = music.catalogue.lock().await;
    let track = music.record(id).await?.track()?;
    if track.uploader_id != auth.user_id
        && !permissions::allows(&auth.role, Permission::ModerateVoice)
    {
        return Err(ServerError::Forbidden);
    }
    music.evict_track(&state.sessions, id);
    music.delete_file(&track).await?;
    Ok(StatusCode::NO_CONTENT)
}
