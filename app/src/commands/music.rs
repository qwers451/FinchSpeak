//! Room music commands: the library, uploads from this computer, and queue control.
//! Uploads send the file as a raw IPC body; the name and the keep flag travel in headers.

use std::path::Path;
use std::sync::atomic::Ordering;

use finch_audio::music::TrackInfo;
use finch_client_core::api::ApiClient;
use finch_protocol::messages::{ClientMessage, MusicCommand};
use finch_protocol::models::MusicTrack;
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

/// Upper bound checked before the file is read; the server enforces its own, smaller, limit too.
const MAX_UPLOAD_BYTES: usize = 500 * 1024 * 1024;
const INVALID_FILENAME: &str = "Invalid music filename";

async fn api_and_generation(state: &AppState) -> Result<(ApiClient, u64), String> {
    let generation = state.session_generation.load(Ordering::SeqCst);
    let api = state.api.read().await.clone().ok_or("Not authenticated")?;
    Ok((api, generation))
}

fn ensure_same_session(state: &AppState, generation: u64) -> Result<(), String> {
    if state.session_generation.load(Ordering::SeqCst) == generation {
        Ok(())
    } else {
        Err("Session changed".into())
    }
}

/// Decodes the `%XX` escapes the browser puts in the filename header, so any Unicode name survives.
fn percent_decode(encoded: &str) -> Result<String, String> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3).ok_or(INVALID_FILENAME)?;
            let text = std::str::from_utf8(hex).map_err(|_| INVALID_FILENAME)?;
            out.push(u8::from_str_radix(text, 16).map_err(|_| INVALID_FILENAME)?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| INVALID_FILENAME.into())
}

/// The last path component of the decoded name, so a file name can never become a path here.
fn upload_filename(encoded: &str) -> Result<String, String> {
    if encoded.is_empty() || encoded.len() > 4096 {
        return Err(INVALID_FILENAME.into());
    }
    let decoded = percent_decode(encoded)?;
    let name = decoded
        .rsplit(['/', '\\'])
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or(INVALID_FILENAME)?;
    Ok(name.chars().take(255).collect())
}

pub(crate) struct UploadHeaders {
    pub filename: String,
    pub keep: bool,
}

/// `x-finch-file-name` is required; `x-finch-keep` is optional and means "false" when absent.
pub(crate) fn upload_headers(headers: &HeaderMap) -> Result<UploadHeaders, String> {
    let encoded = headers
        .get("x-finch-file-name")
        .ok_or(INVALID_FILENAME)?
        .to_str()
        .map_err(|_| INVALID_FILENAME)?;
    let keep = match headers.get("x-finch-keep").map(|value| value.to_str()) {
        None => false,
        Some(Ok("true")) => true,
        Some(Ok("false")) => false,
        Some(_) => return Err("Invalid music upload mode".into()),
    };
    Ok(UploadHeaders { filename: upload_filename(encoded)?, keep })
}

fn fallback_title(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("Music")
        .chars()
        .take(120)
        .collect()
}

/// Tags first; without a title the file name (minus its extension) is used. An empty artist is no artist.
pub(crate) fn track_metadata(info: &TrackInfo, filename: &str) -> (String, Option<String>) {
    let title = info
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| title.chars().take(120).collect::<String>())
        .unwrap_or_else(|| fallback_title(filename));
    let artist = info
        .artist
        .as_deref()
        .map(str::trim)
        .filter(|artist| !artist.is_empty())
        .map(|artist| artist.chars().take(120).collect::<String>());
    (title, artist)
}

#[tauri::command]
pub async fn music_library(state: State<'_, AppState>) -> Result<Vec<MusicTrack>, String> {
    let (api, generation) = api_and_generation(&state).await?;
    let tracks = api.music_library().await.map_err(|e| e.to_string())?;
    ensure_same_session(&state, generation)?;
    Ok(tracks)
}

#[tauri::command]
pub async fn music_delete_track(track_id: Uuid, state: State<'_, AppState>) -> Result<(), String> {
    let (api, generation) = api_and_generation(&state).await?;
    api.delete_music_track(track_id).await.map_err(|e| e.to_string())?;
    ensure_same_session(&state, generation)
}

#[tauri::command]
pub async fn music_command(command: MusicCommand, state: State<'_, AppState>) -> Result<(), String> {
    if state.current_voice_channel.read().await.is_none() {
        return Err("Not in a voice channel".into());
    }
    let sender = state.control_tx.read().await.clone().ok_or("Not connected via QUIC")?;
    sender
        .send(ClientMessage::MusicCommand { command })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn music_set_volume(volume: f32, state: State<'_, AppState>) {
    state.music.set_volume(volume);
}

#[tauri::command]
pub async fn music_upload(request: Request<'_>, state: State<'_, AppState>) -> Result<MusicTrack, String> {
    let headers = upload_headers(request.headers())?;
    let InvokeBody::Raw(data) = request.body() else {
        return Err("Music upload requires binary bytes".into());
    };
    if data.is_empty() || data.len() > MAX_UPLOAD_BYTES {
        return Err("Music file is too large".into());
    }
    let (api, generation) = api_and_generation(&state).await?;
    let owned = data.clone();
    let (owned, info) = tokio::task::spawn_blocking(move || {
        let info = finch_audio::music::probe(&owned);
        (owned, info)
    })
    .await
    .map_err(|e| e.to_string())?;
    let info = info?;
    ensure_same_session(&state, generation)?;
    let (title, artist) = track_metadata(&info, &headers.filename);
    let track = api
        .upload_music_track(owned, &headers.filename, &title, artist.as_deref(), info.duration_ms, headers.keep)
        .await
        .map_err(|e| e.to_string())?;
    ensure_same_session(&state, generation)?;
    Ok(track)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).expect("ASCII header value"));
        }
        map
    }

    #[test]
    fn music_upload_keep_defaults_to_false_and_accepts_only_true_or_false() {
        let name = [("x-finch-file-name", "song.mp3")];
        assert!(!upload_headers(&headers(&name)).expect("valid headers").keep);
        let kept = [("x-finch-file-name", "song.mp3"), ("x-finch-keep", "true")];
        assert!(upload_headers(&headers(&kept)).expect("valid headers").keep);
        let bad = [("x-finch-file-name", "song.mp3"), ("x-finch-keep", "yes")];
        assert!(upload_headers(&headers(&bad)).is_err());
    }

    #[test]
    fn music_upload_requires_a_filename_header() {
        assert!(upload_headers(&HeaderMap::new()).is_err());
        assert!(upload_headers(&headers(&[("x-finch-file-name", "")])).is_err());
    }

    #[test]
    fn music_filename_keeps_unicode_and_cannot_become_a_storage_path() {
        let name = upload_filename("C%3A%5CUsers%5Ctest%5C%D0%9C%D1%83%D0%B7%D1%8B%D0%BA%D0%B0.wav").expect("valid name");
        assert_eq!(name, "Музыка.wav");
        assert!(upload_filename("%FF").is_err());
        assert!(upload_filename("%2").is_err());
    }

    #[test]
    fn music_metadata_falls_back_to_the_file_name_and_drops_an_empty_artist() {
        let info = TrackInfo { duration_ms: 1_000, title: Some("  ".into()), artist: Some(" ".into()) };
        let (title, artist) = track_metadata(&info, "Музыка.wav");
        assert_eq!(title, "Музыка");
        assert_eq!(artist, None);
        let tagged = TrackInfo { duration_ms: 1_000, title: Some("Tune".into()), artist: Some("Band".into()) };
        assert_eq!(track_metadata(&tagged, "x.mp3"), ("Tune".to_string(), Some("Band".to_string())));
    }
}
