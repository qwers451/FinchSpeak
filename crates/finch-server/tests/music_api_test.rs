use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use finch_server::api::create_router;
use finch_server::config::{RegistrationMode, ServerConfig};
use finch_server::db::init_db;
use finch_server::session::SessionManager;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TRACKS: &str = "/api/music/tracks";
const WAV: &[u8] = b"RIFF\x04\x00\x00\x00WAVE";
const MIB: usize = 1024 * 1024;

struct Server {
    app: axum::Router,
    storage: tempfile::TempDir,
    pool: sqlx::SqlitePool,
    sessions: SessionManager,
}

impl Server {
    async fn new(configure: impl FnOnce(&mut toml::Value)) -> anyhow::Result<Self> {
        let storage = tempfile::tempdir()?;
        let mut settings: toml::Value = toml::from_str("[music]")?;
        configure(&mut settings["music"]);
        let mut config: ServerConfig = settings.try_into()?;
        config.storage.upload_dir = storage.path().to_string_lossy().into_owned();
        config.auth.registration = RegistrationMode::Open;
        let pool = init_db("sqlite::memory:").await?;
        let sessions = SessionManager::new();
        Ok(Self {
            app: create_router(pool.clone(), config, sessions.clone()),
            storage,
            pool,
            sessions,
        })
    }

    async fn register(&self, username: &str) -> anyhow::Result<String> {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::post("/api/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"username": username, "password": "long-pass-123"}).to_string(),
                    ))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::CREATED);
        Ok(body_json(response).await?["token"]
            .as_str()
            .unwrap()
            .to_owned())
    }

    async fn request(
        &self,
        method: &str,
        uri: &str,
        token: Option<&str>,
    ) -> anyhow::Result<Response> {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        Ok(self
            .app
            .clone()
            .oneshot(request.body(Body::empty())?)
            .await?)
    }

    async fn upload(
        &self,
        token: &str,
        keep: bool,
        title: &str,
        artist: &str,
        duration: u64,
        data: &[u8],
    ) -> anyhow::Result<Response> {
        let request = upload_request(token, keep, title, artist, duration, data, true)?;
        Ok(self.app.clone().oneshot(request).await?)
    }
}

fn upload_request(
    token: &str,
    keep: bool,
    title: &str,
    artist: &str,
    duration: u64,
    data: &[u8],
    complete: bool,
) -> anyhow::Result<Request<Body>> {
    let mut multipart = Vec::new();
    for (name, value) in [
        ("keep", keep.to_string()),
        ("title", title.to_owned()),
        ("artist", artist.to_owned()),
        ("duration_ms", duration.to_string()),
    ] {
        multipart.extend_from_slice(
            format!("--test\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    multipart.extend_from_slice(b"--test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"../../wrong.mp3\"\r\nContent-Type: audio/mpeg\r\n\r\n");
    multipart.extend_from_slice(data);
    if complete {
        multipart.extend_from_slice(b"\r\n--test--\r\n");
    }
    Ok(Request::post(TRACKS)
        .header(header::CONTENT_TYPE, "multipart/form-data; boundary=test")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(multipart))?)
}

async fn body_json(response: Response) -> anyhow::Result<Value> {
    let body = response.into_body().collect().await?.to_bytes();
    Ok(serde_json::from_slice(&body)?)
}

#[tokio::test]
async fn disabled_music_is_absent_and_enabled_routes_require_authentication() -> anyhow::Result<()>
{
    let enabled = Server::new(|_| {}).await?;
    assert_eq!(
        enabled.request("GET", TRACKS, None).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let disabled = Server::new(|music| {
        music
            .as_table_mut()
            .unwrap()
            .insert("enabled".into(), false.into());
    })
    .await?;
    let token = disabled.register("alice").await?;
    assert_eq!(
        disabled
            .request("GET", TRACKS, Some(&token))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test]
async fn signature_controls_format_and_download_mime_while_temporary_uploads_stay_private()
-> anyhow::Result<()> {
    let server = Server::new(|_| {}).await?;
    let alice = server.register("alice").await?;
    let bob = server.register("bob").await?;
    for (keep, format, mime, sample) in [
        (true, "mp3", "audio/mpeg", &b"ID3track"[..]),
        (true, "mp3", "audio/mpeg", &b"\xff\xfb\x90\x00"[..]),
        (true, "ogg", "audio/ogg", &b"OggStrack"[..]),
        (true, "flac", "audio/flac", &b"fLaCtrack"[..]),
        (false, "wav", "audio/wav", WAV),
    ] {
        let upload = server
            .upload(&alice, keep, "Valid title", "", 1_000, sample)
            .await?;
        assert_eq!(upload.status(), StatusCode::CREATED);
        let track = body_json(upload).await?;
        assert_eq!(track["format"], format);
        assert_eq!(track["temporary"], !keep);
        assert!(track.get("path").is_none());
        let id = uuid::Uuid::parse_str(track["id"].as_str().unwrap())?;
        let file_path = format!("{TRACKS}/{id}/file");
        let download = server.request("GET", &file_path, Some(&alice)).await?;
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(download.headers()[header::CONTENT_TYPE], mime);
        assert_eq!(
            download.headers()[header::CONTENT_LENGTH],
            sample.len().to_string()
        );
        assert_eq!(
            download.into_body().collect().await?.to_bytes().as_ref(),
            sample
        );
        let status = server
            .request("GET", &file_path, Some(&bob))
            .await?
            .status();
        assert_eq!(
            status,
            if keep {
                StatusCode::OK
            } else {
                StatusCode::NOT_FOUND
            }
        );
    }
    let library = body_json(server.request("GET", TRACKS, Some(&bob)).await?).await?;
    assert_eq!(library.as_array().unwrap().len(), 4);
    assert_eq!(
        std::fs::read_dir(server.storage.path())?.count(),
        1,
        "all bytes stay in the dedicated music directory"
    );
    Ok(())
}

#[tokio::test]
async fn validation_checks_bounds_and_rejects_truncation() -> anyhow::Result<()> {
    let server = Server::new(|_| {}).await?;
    let token = server.register("alice").await?;
    for (title, artist, duration, sample) in [
        ("title".to_owned(), String::new(), 999, WAV),
        ("title".to_owned(), String::new(), 10_800_001, WAV),
        ("".to_owned(), String::new(), 1_000, WAV),
        ("音".repeat(121), String::new(), 1_000, WAV),
        ("title".to_owned(), "音".repeat(121), 1_000, WAV),
        ("title".to_owned(), String::new(), 1_000, &b"bad.mp3"[..]),
    ] {
        assert_eq!(
            server
                .upload(&token, true, &title, &artist, duration, sample)
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let truncated = upload_request(&token, true, "Valid title", "", 1_000, WAV, false)?;
    assert_eq!(
        server.app.clone().oneshot(truncated).await?.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        std::fs::read_dir(server.storage.path().join("music"))?.count(),
        0
    );
    let maximum = "音".repeat(120);
    assert_eq!(
        server
            .upload(&token, true, &maximum, &maximum, 10_800_000, WAV)
            .await?
            .status(),
        StatusCode::CREATED
    );
    let mut large = vec![0; 30 * MIB];
    large[..WAV.len()].copy_from_slice(WAV);
    assert_eq!(
        server
            .upload(&token, true, "30 MiB", "", 1_000, &large)
            .await?
            .status(),
        StatusCode::CREATED
    );
    large.push(0);
    assert_eq!(
        server
            .upload(&token, true, "Over 30 MiB", "", 1_000, &large)
            .await?
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        std::fs::read_dir(server.storage.path().join("music"))?.count(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_uploads_cannot_overbook_persistent_or_temporary_quotas() -> anyhow::Result<()> {
    let server = Server::new(|music| {
        music
            .as_table_mut()
            .unwrap()
            .insert("library_quota_mb".into(), 1.into());
        music
            .as_table_mut()
            .unwrap()
            .insert("temp_quota_mb_per_user".into(), 1.into());
        music
            .as_table_mut()
            .unwrap()
            .insert("temp_quota_mb_total".into(), 1.into());
    })
    .await?;
    let alice = server.register("alice").await?;
    let bob = server.register("bob").await?;
    let mut data = vec![0; 3 * MIB / 4];
    data[..WAV.len()].copy_from_slice(WAV);
    for keep in [true, false] {
        let (first, second) = tokio::join!(
            server.upload(&alice, keep, "Alice's track", "", 1_000, &data),
            server.upload(&bob, keep, "Bob's track", "", 1_000, &data),
        );
        let statuses = [first?.status(), second?.status()];
        assert!(statuses.contains(&StatusCode::CREATED));
        assert!(statuses.contains(&StatusCode::PAYLOAD_TOO_LARGE));
    }
    assert_eq!(
        std::fs::read_dir(server.storage.path().join("music"))?.count(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn upload_rate_limit_and_deletion_follow_user_identity() -> anyhow::Result<()> {
    let server = Server::new(|_| {}).await?;
    let staff = server.register("admin").await?;
    let alice = server.register("alice").await?;
    let bob = server.register("bob").await?;
    for index in 0..10 {
        let upload = server.upload(&alice, true, "Track", "", 1_000, WAV).await?;
        assert_eq!(upload.status(), StatusCode::CREATED);
        if index < 2 {
            let track = body_json(upload).await?;
            let path = format!("{TRACKS}/{}", track["id"].as_str().unwrap());
            assert_eq!(
                server.request("DELETE", &path, Some(&bob)).await?.status(),
                StatusCode::FORBIDDEN
            );
            let authorized = if index == 0 { &staff } else { &alice };
            assert_eq!(
                server
                    .request("DELETE", &path, Some(authorized))
                    .await?
                    .status(),
                StatusCode::NO_CONTENT
            );
            assert_eq!(
                server
                    .request("GET", &format!("{path}/file"), Some(authorized))
                    .await?
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
    }
    assert_eq!(
        server
            .upload(&alice, true, "Rate limited", "", 1_000, WAV)
            .await?
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        server
            .upload(&bob, true, "Separate limit", "", 1_000, WAV)
            .await?
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        std::fs::read_dir(server.storage.path().join("music"))?.count(),
        9
    );
    Ok(())
}

#[tokio::test]
async fn database_paths_cannot_redirect_music_download_or_deletion() -> anyhow::Result<()> {
    let server = Server::new(|_| {}).await?;
    let token = server.register("alice").await?;
    let response = server.upload(&token, true, "Track", "", 1_000, WAV).await?;
    let track = body_json(response).await?;
    let id = track["id"].as_str().unwrap();
    let outside = server.storage.path().join("private-data");
    std::fs::write(&outside, b"private")?;
    sqlx::query("UPDATE music_tracks SET path = ? WHERE id = ?")
        .bind(outside.to_string_lossy().as_ref())
        .bind(id)
        .execute(&server.pool)
        .await?;
    let downloaded = server
        .request("GET", &format!("{TRACKS}/{id}/file"), Some(&token))
        .await?
        .into_body()
        .collect()
        .await?
        .to_bytes();
    assert_eq!(downloaded.as_ref(), WAV);
    assert_eq!(
        server
            .request("DELETE", &format!("{TRACKS}/{id}"), Some(&token))
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(std::fs::read(outside)?, b"private");
    assert!(server.sessions.music.get().is_some());
    Ok(())
}
