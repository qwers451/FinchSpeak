//! File attachment upload and download endpoints.

use std::path::PathBuf;

use axum::extract::{Multipart, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use finch_protocol::models::AttachmentInfo;
use tokio::fs;
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;

/// Handler for POST /api/files/upload
pub async fn upload_file_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ServerError> {
    let upload_dir = PathBuf::from(&state.config.storage.upload_dir);
    if !upload_dir.exists() {
        fs::create_dir_all(&upload_dir)
            .await
            .map_err(|e| ServerError::Internal(format!("Failed to create upload dir: {e}")))?;
    }

    let max_bytes = state.config.storage.max_file_size_mb * 1024 * 1024;

    if let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ServerError::BadRequest(e.to_string()))?
    {
        let filename = field
            .file_name()
            .unwrap_or("unnamed_attachment")
            .to_string();

        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        let data = field
            .bytes()
            .await
            .map_err(|e| ServerError::BadRequest(e.to_string()))?;

        if data.len() > max_bytes {
            return Err(ServerError::FileUpload(format!(
                "File exceeds maximum allowed size of {} MB",
                state.config.storage.max_file_size_mb
            )));
        }

        let attachment_id = Uuid::new_v4();
        let extension = PathBuf::from(&filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("bin")
            .to_string();

        let safe_filename = format!("{}.{}", attachment_id, extension);
        let file_path = upload_dir.join(&safe_filename);

        fs::write(&file_path, &data)
            .await
            .map_err(|e| ServerError::Internal(format!("Failed to write file to disk: {e}")))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        sqlx::query(
            "INSERT INTO attachments (id, uploader_id, filename, size_bytes, content_type, storage_path, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(attachment_id.to_string())
        .bind(auth.user_id.to_string())
        .bind(&filename)
        .bind(data.len() as i64)
        .bind(&content_type)
        .bind(file_path.to_string_lossy().to_string())
        .bind(now)
        .execute(&state.pool)
        .await?;

        let info = AttachmentInfo {
            id: attachment_id,
            filename,
            size_bytes: data.len() as u64,
            content_type,
            url: format!("/api/files/{}", attachment_id),
        };

        return Ok((StatusCode::CREATED, Json(info)));
    }

    Err(ServerError::BadRequest("No file provided in form-data".to_string()))
}

/// Handler for GET /api/files/:id
pub async fn download_file_handler(
    _auth: AuthUser,
    Path(file_id_str): Path<String>,
    State(state): State<ArcSharedState>,
) -> Result<impl IntoResponse, ServerError> {
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT filename, content_type, storage_path FROM attachments WHERE id = ?"
    )
    .bind(&file_id_str)
    .fetch_optional(&state.pool)
    .await?;

    let (filename, content_type, storage_path) = match row {
        Some(r) => r,
        None => return Err(ServerError::NotFound("File not found".to_string())),
    };

    let data = fs::read(&storage_path)
        .await
        .map_err(|_| ServerError::NotFound("File not found on disk".to_string()))?;

    let mut headers = HeaderMap::new();
    let content_type_header = HeaderValue::from_str(&content_type)
        .map_err(|e| ServerError::Internal(format!("Invalid Content-Type header: {e}")))?;
    let disposition_str = format!("inline; filename=\"{filename}\"");
    let disposition_header = HeaderValue::from_str(&disposition_str)
        .map_err(|e| ServerError::Internal(format!("Invalid Content-Disposition header: {e}")))?;

    headers.insert(CONTENT_TYPE, content_type_header);
    headers.insert(CONTENT_DISPOSITION, disposition_header);

    Ok((headers, data))
}
