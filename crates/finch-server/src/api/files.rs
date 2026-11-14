//! File attachment upload and download endpoints.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use finch_protocol::models::AttachmentInfo;
use tokio::fs;
use uuid::Uuid;

use crate::api::auth::{ArcSharedState, AuthUser};
use crate::error::ServerError;

/// Room for multipart boundaries and headers on top of the file itself.
const MULTIPART_OVERHEAD_BYTES: usize = 64 * 1024;

/// How much of an oversized upload is read and discarded so the client can receive the 413.
fn drain_cap(max_bytes: usize) -> usize {
    max_bytes.saturating_mul(4).max(16 * 1024 * 1024)
}

/// Request body limit of the upload route: the drain cap plus multipart framing.
pub fn upload_body_limit(max_file_size_mb: usize) -> usize {
    drain_cap(max_file_size_mb * 1024 * 1024) + MULTIPART_OVERHEAD_BYTES
}

/// A file being written next to its final place; whatever ends the request early (an error, a client
/// that went away and drops the handler) removes it.
struct PartFile {
    path: PathBuf,
    armed: bool,
}

impl PartFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    /// The file has become the real one: keep it.
    fn keep(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

const MIB: u64 = 1024 * 1024;

/// Images up to this size get a preview made from the stored file.
const THUMBNAIL_MAX_BYTES: u64 = 16 * MIB;

fn too_large(storage: &crate::config::StorageConfig) -> ServerError {
    ServerError::FileUpload(format!("File exceeds maximum allowed size of {} MB", storage.max_file_size_mb))
}

fn over_quota(storage: &crate::config::StorageConfig) -> ServerError {
    ServerError::FileUpload(format!(
        "Storage quota exceeded: each user may keep up to {} MB of files",
        storage.user_quota_mb
    ))
}

/// Handler for POST /api/files/upload
pub async fn upload_file_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ServerError> {
    let upload_dir = PathBuf::from(&state.config.storage.upload_dir);
    if !upload_dir.exists() {
        fs::create_dir_all(&upload_dir)
            .await
            .map_err(|e| ServerError::Internal(format!("Failed to create upload dir: {e}")))?;
    }

    let storage = &state.config.storage;
    let max_bytes = (storage.max_file_size_mb as u64) * MIB;
    let quota_bytes = (storage.user_quota_mb as u64) * MIB;

    // A refusal is held back until the body has been read (up to `drain_cap`): answering and closing
    // while the client is still sending makes it see a connection reset instead of the response.
    let mut rejected: Option<ServerError> = None;
    if let Err(wait) = state.sessions.allow_action(auth.user_id, "upload", storage.uploads_per_minute, std::time::Duration::from_secs(60)) {
        rejected = Some(ServerError::TooManyUploads(wait));
    }
    let used: u64 = sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(size_bytes), 0) FROM attachments WHERE uploader_id = ?")
        .bind(auth.user_id.to_string())
        .fetch_one(&state.pool)
        .await?
        .max(0) as u64;
    // The announced body length (minus multipart framing) is checked before a byte is stored.
    let announced = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|length| length.saturating_sub(MULTIPART_OVERHEAD_BYTES as u64));
    if let (None, Some(length)) = (&rejected, announced) {
        if length > max_bytes {
            rejected = Some(too_large(storage));
        } else if used.saturating_add(length) > quota_bytes {
            rejected = Some(over_quota(storage));
        }
    }

    // A voice message sends its length and waveform as text fields before the file.
    let mut voice = VoiceFields::default();
    let first = loop {
        let Some(field) = multipart
            .next_field()
            .await
            .map_err(|e| ServerError::BadRequest(e.to_string()))?
        else {
            break None;
        };
        let name = field.name().map(str::to_string);
        match name.as_deref() {
            Some(VOICE_DURATION_FIELD) | Some(VOICE_WAVEFORM_FIELD) => {
                let text = field.text().await.map_err(|e| ServerError::BadRequest(e.to_string()))?;
                voice.set(name.as_deref().unwrap_or_default(), text);
            }
            _ => break Some(field),
        }
    };

    if let Some(mut field) = first {
        let filename = field
            .file_name()
            .unwrap_or("unnamed_attachment")
            .to_string();

        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        let (duration_ms, waveform) = if content_type == "audio/ogg" {
            voice.parse()?
        } else {
            (None, None)
        };

        let attachment_id = Uuid::new_v4();
        let extension = storage_extension(&filename);
        let safe_filename = format!("{}.{}", attachment_id, extension);
        let file_path = upload_dir.join(&safe_filename);
        let mut part = PartFile::new(upload_dir.join(format!("{attachment_id}.part")));

        // The body goes to disk chunk by chunk: memory use does not depend on the file size.
        let drain_cap = drain_cap(max_bytes as usize) as u64;
        let mut file = if rejected.is_none() {
            Some(
                fs::File::create(&part.path)
                    .await
                    .map_err(|e| ServerError::Internal(format!("Failed to create file on disk: {e}")))?,
            )
        } else {
            None
        };
        let mut received = 0u64;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| ServerError::BadRequest(e.to_string()))?
        {
            received += chunk.len() as u64;
            if received > drain_cap {
                break;
            }
            if rejected.is_some() {
                continue;
            }
            if received > max_bytes {
                rejected = Some(too_large(storage));
            } else if used.saturating_add(received) > quota_bytes {
                rejected = Some(over_quota(storage));
            } else if let Some(out) = file.as_mut() {
                out.write_all(&chunk)
                    .await
                    .map_err(|e| ServerError::Internal(format!("Failed to write file to disk: {e}")))?;
                continue;
            }
            // Refused: close and delete the half-written file now, then keep reading and dropping the rest.
            file = None;
            let _ = fs::remove_file(&part.path).await;
        }
        if received > max_bytes && rejected.is_none() {
            rejected = Some(too_large(storage));
        }
        if let Some(error) = rejected {
            return Err(error);
        }
        let Some(mut out) = file else {
            return Err(ServerError::Internal("Upload file handle lost".to_string()));
        };
        out.flush().await.map_err(|e| ServerError::Internal(format!("Failed to write file to disk: {e}")))?;
        out.sync_all().await.map_err(|e| ServerError::Internal(format!("Failed to sync file to disk: {e}")))?;
        drop(out);
        fs::rename(&part.path, &file_path)
            .await
            .map_err(|e| ServerError::Internal(format!("Failed to store file: {e}")))?;
        part.keep();
        // From here the stored file goes away again unless it is recorded in the database.
        let mut stored = PartFile::new(file_path.clone());

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Images get a preview; anything that does not decode stays a plain file.
        let preview = if content_type.starts_with("image/") && received <= THUMBNAIL_MAX_BYTES {
            match fs::read(&file_path).await {
                Ok(bytes) => tokio::task::spawn_blocking(move || crate::chat::make_thumbnail(&bytes)).await.ok().flatten(),
                Err(_) => None,
            }
        } else {
            None
        };
        let (width, height, thumb_path) = match preview {
            Some((width, height, thumb)) => {
                let thumb_file = upload_dir.join(format!("{attachment_id}.thumb"));
                match fs::write(&thumb_file, &thumb).await {
                    Ok(()) => (Some(width), Some(height), Some(thumb_file.to_string_lossy().to_string())),
                    Err(_) => (Some(width), Some(height), None),
                }
            }
            None => (None, None, None),
        };

        sqlx::query(
            "INSERT INTO attachments (id, uploader_id, filename, size_bytes, content_type, storage_path, created_at, width, height, thumb_path, duration_ms, waveform)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(attachment_id.to_string())
        .bind(auth.user_id.to_string())
        .bind(&filename)
        .bind(received as i64)
        .bind(&content_type)
        .bind(file_path.to_string_lossy().to_string())
        .bind(now)
        .bind(width.map(i64::from))
        .bind(height.map(i64::from))
        .bind(&thumb_path)
        .bind(duration_ms)
        .bind(waveform.clone())
        .execute(&state.pool)
        .await?;
        stored.keep();

        let info = AttachmentInfo {
            id: attachment_id,
            filename,
            size_bytes: received,
            content_type,
            url: format!("/api/files/{}", attachment_id),
            width,
            height,
            has_thumb: thumb_path.is_some(),
            duration_ms: duration_ms.map(|ms| ms as u64),
            waveform,
        };

        return Ok((StatusCode::CREATED, Json(info)));
    }

    Err(rejected.unwrap_or_else(|| ServerError::BadRequest("No file provided in form-data".to_string())))
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

    let file = fs::File::open(&storage_path)
        .await
        .map_err(|_| ServerError::NotFound("File not found on disk".to_string()))?;
    let length = file
        .metadata()
        .await
        .map_err(|_| ServerError::NotFound("File not found on disk".to_string()))?
        .len();

    let mut headers = HeaderMap::new();
    let content_type_header = HeaderValue::from_str(&safe_content_type(&content_type))
        .map_err(|e| ServerError::Internal(format!("Invalid Content-Type header: {e}")))?;
    let disposition_str = content_disposition(&filename, &safe_content_type(&content_type));
    let disposition_header = HeaderValue::from_str(&disposition_str)
        .map_err(|e| ServerError::Internal(format!("Invalid Content-Disposition header: {e}")))?;

    headers.insert(CONTENT_TYPE, content_type_header);
    headers.insert(CONTENT_DISPOSITION, disposition_header);
    headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
    // Whatever was uploaded must never run or be re-interpreted as something else when opened.
    headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    headers.insert("Content-Security-Policy", HeaderValue::from_static("default-src 'none'; sandbox"));

    // The file is read and sent in 64 KiB pieces.
    let body = Body::from_stream(ReaderStream::with_capacity(file, 64 * 1024));
    Ok((headers, body))
}

/// Handler for GET /api/files/:id/thumb: the 320 px preview of an image attachment.
pub async fn download_thumb_handler(
    _auth: AuthUser,
    Path(file_id_str): Path<String>,
    State(state): State<ArcSharedState>,
) -> Result<impl IntoResponse, ServerError> {
    let path: Option<String> = sqlx::query_scalar("SELECT thumb_path FROM attachments WHERE id = ?")
        .bind(&file_id_str)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    let path = path.ok_or_else(|| ServerError::NotFound("No preview for this file".to_string()))?;
    let data = fs::read(&path).await.map_err(|_| ServerError::NotFound("Preview not found on disk".to_string()))?;
    let content_type = if data.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    Ok((headers, data))
}

const VOICE_DURATION_FIELD: &str = "duration_ms";
const VOICE_WAVEFORM_FIELD: &str = "waveform";
/// The longest voice message the app records, in milliseconds.
const MAX_VOICE_MS: u64 = 120_000;
/// Values in the waveform of a voice message, one byte each.
const WAVEFORM_BYTES: usize = 64;

/// The text fields of a voice message, as they arrived.
#[derive(Default)]
struct VoiceFields {
    duration_ms: Option<String>,
    waveform: Option<String>,
}

impl VoiceFields {
    fn set(&mut self, name: &str, text: String) {
        match name {
            VOICE_DURATION_FIELD => self.duration_ms = Some(text),
            VOICE_WAVEFORM_FIELD => self.waveform = Some(text),
            _ => {}
        }
    }

    /// The checked length and waveform; a missing field stays empty.
    fn parse(&self) -> Result<(Option<i64>, Option<Vec<u8>>), ServerError> {
        let duration_ms = match &self.duration_ms {
            Some(text) => {
                let value: u64 = text
                    .trim()
                    .parse()
                    .map_err(|_| ServerError::BadRequest("duration_ms must be a whole number".to_string()))?;
                if value > MAX_VOICE_MS {
                    return Err(ServerError::BadRequest("A voice message is at most 2 minutes long".to_string()));
                }
                Some(value as i64)
            }
            None => None,
        };
        let waveform = match &self.waveform {
            Some(hex) => Some(decode_waveform(hex)?),
            None => None,
        };
        Ok((duration_ms, waveform))
    }
}

/// A waveform arrives as hex text: 64 bytes, so 128 hex characters.
pub fn decode_waveform(hex: &str) -> Result<Vec<u8>, ServerError> {
    let hex = hex.trim();
    let invalid = || ServerError::BadRequest(format!("waveform must be {WAVEFORM_BYTES} bytes in hex"));
    if hex.len() != WAVEFORM_BYTES * 2 || !hex.is_ascii() {
        return Err(invalid());
    }
    hex.as_bytes()
        .chunks(2)
        .map(|pair| std::str::from_utf8(pair).ok().and_then(|text| u8::from_str_radix(text, 16).ok()))
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(invalid)
}

#[cfg(test)]
mod voice_note_tests {
    use super::*;

    #[test]
    fn voice_note_waveform_must_be_exactly_64_bytes() {
        let good = "ff".repeat(WAVEFORM_BYTES);
        assert_eq!(decode_waveform(&good).unwrap(), vec![0xff; WAVEFORM_BYTES]);
        assert!(decode_waveform(&"ff".repeat(63)).is_err(), "63 values are refused");
        assert!(decode_waveform(&"ff".repeat(65)).is_err(), "65 values are refused");
        assert!(decode_waveform(&format!("{}zz", "ff".repeat(63))).is_err(), "non-hex text is refused");
    }

    #[test]
    fn voice_note_duration_is_capped_at_two_minutes() {
        let mut fields = VoiceFields::default();
        fields.set(VOICE_DURATION_FIELD, "120000".to_string());
        assert_eq!(fields.parse().unwrap().0, Some(120_000));
        fields.set(VOICE_DURATION_FIELD, "120001".to_string());
        assert!(fields.parse().is_err());
        fields.set(VOICE_DURATION_FIELD, "soon".to_string());
        assert!(fields.parse().is_err());
    }

    #[test]
    fn voice_note_fields_are_optional() {
        assert_eq!(VoiceFields::default().parse().unwrap(), (None, None));
    }
}

/// File extension used for the name on disk: letters and digits only, at most 8, else `bin`.
pub fn storage_extension(filename: &str) -> String {
    let ext = filename.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    if ext.is_empty() || ext.len() > 8 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return "bin".to_string();
    }
    ext.to_lowercase()
}

/// Only pictures open inline (and only the kinds a browser shows safely); everything else is downloaded.
pub fn content_disposition(filename: &str, content_type: &str) -> String {
    let inline_ok = matches!(content_type, "image/png" | "image/jpeg" | "image/gif" | "image/webp");
    let name: String = filename.chars().map(|c| if c.is_control() || matches!(c, '"' | '\\' | '/' | ';') { '_' } else { c }).collect();
    format!("{}; filename=\"{}\"", if inline_ok { "inline" } else { "attachment" }, name)
}

/// The type announced by the uploader, unless it is not a plain `type/subtype`: then it is opaque bytes.
pub fn safe_content_type(announced: &str) -> String {
    let plain = announced.len() <= 100
        && announced.matches('/').count() == 1
        && !announced.starts_with('/')
        && !announced.ends_with('/')
        && announced.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '+' | '-' | '_'));
    if plain { announced.to_ascii_lowercase() } else { "application/octet-stream".to_string() }
}

#[cfg(test)]
mod hardening_tests {
    use super::*;

    #[test]
    fn only_simple_extensions_reach_the_disk_name() {
        assert_eq!(storage_extension("report.PDF"), "pdf");
        assert_eq!(storage_extension("archive.tar.gz"), "gz");
        assert_eq!(storage_extension(".hidden"), "hidden");
        let long = format!("a.{}", "x".repeat(30));
        for odd in ["noextension", "x.", "a.b c", "a.<script>", "evil.exe.", "a./../b", long.as_str(), "a.\u{0}"] {
            assert_eq!(storage_extension(odd), "bin", "{odd:?}");
        }
    }

    #[test]
    fn pictures_open_inline_and_everything_else_is_a_download() {
        assert!(content_disposition("a.png", "image/png").starts_with("inline"));
        for risky in ["text/html", "image/svg+xml", "application/pdf", "application/octet-stream", "text/javascript"] {
            assert!(content_disposition("a.x", risky).starts_with("attachment"), "{risky}");
        }
    }

    #[test]
    fn the_file_name_cannot_break_out_of_its_quotes() {
        let header = content_disposition("a\"; filename=\"evil.exe\r\nX-Injected: 1", "text/plain");
        assert!(!header.contains('\r') && !header.contains('\n'));
        assert_eq!(header.matches('"').count(), 2);
        assert!(HeaderValue::from_str(&header).is_ok());
    }

    #[test]
    fn announced_types_are_plain_or_opaque() {
        assert_eq!(safe_content_type("Image/PNG"), "image/png");
        let long = "a/".repeat(80);
        for bad in ["", "text", "text/html; charset=x", "a b/c", "text/html\r\nX: y", long.as_str(), "/x", "x/"] {
            assert_eq!(safe_content_type(bad), "application/octet-stream", "{bad:?}");
        }
    }
}
