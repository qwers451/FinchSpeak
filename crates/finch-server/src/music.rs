//! Room music storage. The server stores and forwards files without decoding audio.
pub mod queue;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use finch_protocol::models::MusicTrack;
use sqlx::SqlitePool;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::config::ServerConfig;
use crate::error::ServerError;
use crate::limits::WindowLimiter;

pub const MIB: u64 = 1024 * 1024;
pub const TEMP_LEASE_SECS: i64 = 15 * 60;
const SELECT_TRACK: &str = "SELECT id, uploader_id, title, artist, duration_ms, size_bytes, format, temporary, created_at, queued_in FROM music_tracks";

/// Serializes quota accounting, publishing files, deletion, and queue leases.
/// Room locks are never held during this service's database or disk operations.
pub struct MusicService {
    pub pool: SqlitePool,
    pub config: crate::config::MusicConfig,
    pub dir: PathBuf,
    pub catalogue: Mutex<()>,
    pending: DashMap<Uuid, Reservation>,
    pub upload_hits: WindowLimiter,
    pub command_hits: WindowLimiter,
}

#[derive(Clone, Copy)]
struct Reservation {
    owner: Uuid,
    keep: bool,
    bytes: u64,
}

#[derive(sqlx::FromRow)]
pub struct TrackRow {
    id: String,
    uploader_id: String,
    title: String,
    artist: Option<String>,
    duration_ms: i64,
    size_bytes: i64,
    format: String,
    temporary: bool,
    created_at: i64,
    pub queued_in: Option<i64>,
}

impl TrackRow {
    pub fn track(self) -> Result<MusicTrack, ServerError> {
        Ok(MusicTrack {
            id: Uuid::parse_str(&self.id).map_err(|e| ServerError::Internal(e.to_string()))?,
            uploader_id: Uuid::parse_str(&self.uploader_id)
                .map_err(|e| ServerError::Internal(e.to_string()))?,
            title: self.title,
            artist: self.artist,
            duration_ms: self.duration_ms as u64,
            size_bytes: self.size_bytes as u64,
            format: self.format,
            temporary: self.temporary,
            created_at: self.created_at,
        })
    }
}

impl MusicService {
    pub fn new(pool: SqlitePool, config: &ServerConfig) -> Self {
        let dir = config.music.storage_dir.as_ref().map_or_else(
            || Path::new(&config.storage.upload_dir).join("music"),
            PathBuf::from,
        );
        Self {
            pool,
            config: config.music.clone(),
            dir,
            catalogue: Mutex::new(()),
            pending: DashMap::new(),
            upload_hits: WindowLimiter::default(),
            command_hits: WindowLimiter::default(),
        }
    }

    pub fn require_enabled(&self) -> Result<(), ServerError> {
        if self.config.enabled {
            Ok(())
        } else {
            Err(ServerError::NotFound("Music is disabled".into()))
        }
    }

    pub fn file_path(&self, id: Uuid, format: &str) -> Result<PathBuf, ServerError> {
        mime(format)?;
        Ok(self.dir.join(format!("{id}.{format}")))
    }

    pub async fn record(&self, id: Uuid) -> Result<TrackRow, ServerError> {
        sqlx::query_as(&format!("{SELECT_TRACK} WHERE id = ? AND deleting = 0"))
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ServerError::NotFound("Music track not found".into()))
    }

    pub async fn library(&self) -> Result<Vec<MusicTrack>, ServerError> {
        let rows: Vec<TrackRow> = sqlx::query_as(&format!(
            "{SELECT_TRACK} WHERE temporary = 0 AND deleting = 0 ORDER BY created_at DESC, id DESC"
        ))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(TrackRow::track).collect()
    }

    pub fn release_reservation(&self, id: Uuid) {
        self.pending.remove(&id);
    }

    /// Every byte, including unfinished uploads, belongs to a bounded quota.
    /// Until a file-first multipart provides keep, it consumes the temporary pool.
    pub async fn reserve(
        &self,
        id: Uuid,
        owner: Uuid,
        keep: bool,
        bytes: u64,
    ) -> Result<(), ServerError> {
        let _guard = self.catalogue.lock().await;
        let committed: Vec<(bool, String, i64)> = sqlx::query_as(
            "SELECT temporary, uploader_id, SUM(size_bytes) FROM music_tracks GROUP BY temporary, uploader_id",
        ).fetch_all(&self.pool).await?;
        let mut library = 0u64;
        let mut temporary = 0u64;
        let mut user_temporary = 0u64;
        for (is_temporary, uploader, count) in committed {
            if is_temporary {
                temporary = temporary.saturating_add(count as u64);
                if uploader == owner.to_string() {
                    user_temporary = user_temporary.saturating_add(count as u64);
                }
            } else {
                library = library.saturating_add(count as u64);
            }
        }
        for reservation in self.pending.iter().filter(|item| *item.key() != id) {
            if reservation.keep {
                library = library.saturating_add(reservation.bytes);
            } else {
                temporary = temporary.saturating_add(reservation.bytes);
                if reservation.owner == owner {
                    user_temporary = user_temporary.saturating_add(reservation.bytes);
                }
            }
        }
        if keep && library.saturating_add(bytes) > self.config.library_quota_mb.saturating_mul(MIB)
        {
            return Err(ServerError::FileUpload(
                "Music library quota exceeded".into(),
            ));
        }
        if !keep
            && (temporary.saturating_add(bytes)
                > self.config.temp_quota_mb_total.saturating_mul(MIB)
                || user_temporary.saturating_add(bytes)
                    > self.config.temp_quota_mb_per_user.saturating_mul(MIB))
        {
            return Err(ServerError::FileUpload(
                "Temporary music quota exceeded".into(),
            ));
        }
        self.pending.insert(id, Reservation { owner, keep, bytes });
        Ok(())
    }

    /// Call while holding catalogue, after queue entries have been evicted.
    /// Mark first so failed disk deletion remains charged and can be retried.
    pub async fn delete_file(&self, track: &MusicTrack) -> Result<(), ServerError> {
        sqlx::query("UPDATE music_tracks SET deleting = 1 WHERE id = ?")
            .bind(track.id.to_string())
            .execute(&self.pool)
            .await?;
        let path = self.file_path(track.id, &track.format)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        sqlx::query("DELETE FROM music_tracks WHERE id = ? AND deleting = 1")
            .bind(track.id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Runs before accepting connections. Persistent library entries survive a restart.
    pub async fn startup_cleanup(&self) -> Result<(), ServerError> {
        let _guard = self.catalogue.lock().await;
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(io_error)?;
        let rows: Vec<TrackRow> = sqlx::query_as(&format!(
            "{SELECT_TRACK} WHERE temporary = 1 OR deleting = 1"
        ))
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let track = row.track()?;
            self.delete_file(&track).await?;
        }
        self.remove_partials().await?;
        let entries: Vec<(String, String)> = sqlx::query_as("SELECT id, format FROM music_tracks")
            .fetch_all(&self.pool)
            .await?;
        let committed: HashSet<_> = entries
            .into_iter()
            .map(|(id, format)| format!("{id}.{format}"))
            .collect();
        let mut files = tokio::fs::read_dir(&self.dir).await.map_err(io_error)?;
        while let Some(file) = files.next_entry().await.map_err(io_error)? {
            let name = file.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((id, format)) = name.rsplit_once('.') else {
                continue;
            };
            if Uuid::parse_str(id).is_ok() && mime(format).is_ok() && !committed.contains(name) {
                tokio::fs::remove_file(file.path())
                    .await
                    .map_err(io_error)?;
            }
        }
        Ok(())
    }

    async fn remove_partials(&self) -> Result<(), ServerError> {
        let mut files = match tokio::fs::read_dir(&self.dir).await {
            Ok(files) => files,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(error)),
        };
        while let Some(file) = files.next_entry().await.map_err(io_error)? {
            let name = file.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(id) = name
                .strip_suffix(".part")
                .and_then(|id| Uuid::parse_str(id).ok())
            else {
                continue;
            };
            if !self.pending.contains_key(&id) {
                match tokio::fs::remove_file(file.path()).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => tracing::warn!("Music partial cleanup failed: {error}"),
                }
            }
        }
        Ok(())
    }

    pub async fn sweep(&self, now: i64) -> Result<(), ServerError> {
        let _guard = self.catalogue.lock().await;
        let rows: Vec<TrackRow> = sqlx::query_as(&format!("{SELECT_TRACK} WHERE deleting = 1 OR (temporary = 1 AND queued_in IS NULL AND created_at <= ?)"))
            .bind(now - TEMP_LEASE_SECS).fetch_all(&self.pool).await?;
        for row in rows {
            let track = row.track()?;
            if let Err(error) = self.delete_file(&track).await {
                tracing::warn!("Music cleanup will retry {}: {error}", track.id);
            }
        }
        self.remove_partials().await
    }
}

/// Owns both reservation and partial file, including cancellation of the HTTP future.
pub struct UploadGuard {
    pub service: Arc<MusicService>,
    pub id: Uuid,
    pub partial: PathBuf,
    pub final_path: Option<PathBuf>,
    pub published: bool,
}

impl UploadGuard {
    pub fn new(service: Arc<MusicService>) -> Self {
        let id = Uuid::new_v4();
        let partial = service.dir.join(format!("{id}.part"));
        Self {
            service,
            id,
            partial,
            final_path: None,
            published: false,
        }
    }

    pub async fn discard(&mut self) {
        for path in std::iter::once(&self.partial).chain(self.final_path.as_ref()) {
            if let Err(error) = tokio::fs::remove_file(path).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!("Music upload cleanup will retry: {error}");
                return;
            }
        }
        self.service.pending.remove(&self.id);
        self.published = true;
    }
}

impl Drop for UploadGuard {
    fn drop(&mut self) {
        if self.published {
            self.service.pending.remove(&self.id);
            return;
        }
        let service = self.service.clone();
        let id = self.id;
        let paths: Vec<_> = std::iter::once(self.partial.clone())
            .chain(self.final_path.clone())
            .collect();
        // Keep the reservation until all owned bytes have been removed, including a cancelled write.
        tokio::spawn(async move {
            for attempt in 0..5 {
                let mut removed = true;
                for path in &paths {
                    if let Err(error) = tokio::fs::remove_file(path).await
                        && error.kind() != std::io::ErrorKind::NotFound
                    {
                        removed = false;
                        tracing::warn!("Music upload cleanup attempt {attempt}: {error}");
                    }
                }
                if removed {
                    service.pending.remove(&id);
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            // Keep failed cleanup charged; the next restart removes the owned partial.
        });
    }
}

pub fn detect_format(bytes: &[u8]) -> Result<&'static str, ServerError> {
    if bytes.starts_with(b"ID3")
        || (bytes.len() >= 4
            && bytes[0] == 0xff
            && bytes[1] & 0xe0 == 0xe0
            && bytes[1] & 0x18 != 0x08
            && bytes[1] & 0x06 != 0
            && bytes[2] & 0xf0 != 0
            && bytes[2] & 0xf0 != 0xf0
            && bytes[2] & 0x0c != 0x0c)
    {
        Ok("mp3")
    } else if bytes.starts_with(b"OggS") {
        Ok("ogg")
    } else if bytes.starts_with(b"fLaC") {
        Ok("flac")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        Ok("wav")
    } else {
        Err(ServerError::BadRequest("Unsupported music format".into()))
    }
}

pub fn mime(format: &str) -> Result<&'static str, ServerError> {
    match format {
        "mp3" => Ok("audio/mpeg"),
        "ogg" => Ok("audio/ogg"),
        "flac" => Ok("audio/flac"),
        "wav" => Ok("audio/wav"),
        _ => Err(ServerError::BadRequest("Unsupported music format".into())),
    }
}

pub fn io_error(error: std::io::Error) -> ServerError {
    ServerError::Internal(format!("Music storage error: {error}"))
}

pub fn validate_metadata(
    fields: &HashMap<String, String>,
    max_minutes: u64,
) -> Result<(String, Option<String>, u64, bool), ServerError> {
    let title = fields.get("title").map(|text| text.trim()).unwrap_or("");
    let artist = fields.get("artist").map(|text| text.trim()).unwrap_or("");
    if title.is_empty() || title.chars().count() > 120 || artist.chars().count() > 120 {
        return Err(ServerError::BadRequest(
            "Music title or artist is invalid".into(),
        ));
    }
    let duration_ms = fields
        .get("duration_ms")
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|duration| *duration >= 1_000 && *duration <= max_minutes.saturating_mul(60_000))
        .ok_or_else(|| ServerError::BadRequest("Music duration is invalid".into()))?;
    let keep = match fields.get("keep").map(String::as_str) {
        Some("true") => true,
        Some("false") => false,
        _ => {
            return Err(ServerError::BadRequest(
                "Music keep must be true or false".into(),
            ));
        }
    };
    Ok((
        title.to_owned(),
        (!artist.is_empty()).then(|| artist.to_owned()),
        duration_ms,
        keep,
    ))
}
