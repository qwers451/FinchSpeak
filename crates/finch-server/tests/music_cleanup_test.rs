use std::borrow::Cow;
use std::sync::Arc;

use finch_protocol::models::MusicTrack;
use finch_server::config::ServerConfig;
use finch_server::music::{MusicService, TEMP_LEASE_SECS};
use uuid::Uuid;

async fn add(
    service: &MusicService,
    temporary: bool,
    queued: bool,
    created_at: i64,
) -> anyhow::Result<MusicTrack> {
    let track = MusicTrack {
        id: Uuid::new_v4(),
        uploader_id: Uuid::new_v4(),
        title: "Music".into(),
        artist: None,
        duration_ms: 1_000,
        size_bytes: 4,
        format: "mp3".into(),
        temporary,
        created_at,
    };
    let path = service.file_path(track.id, &track.format)?;
    tokio::fs::write(&path, b"ID3x").await?;
    sqlx::query("INSERT INTO music_tracks (id, uploader_id, title, duration_ms, size_bytes, format, path, temporary, created_at, queued_in) VALUES (?, ?, 'Music', 1000, 4, 'mp3', ?, ?, ?, ?)")
        .bind(track.id.to_string()).bind(track.uploader_id.to_string()).bind(path.to_string_lossy().as_ref())
        .bind(temporary).bind(created_at).bind(queued.then_some(2i64)).execute(&service.pool).await?;
    Ok(track)
}

#[tokio::test]
async fn cleanup_expires_unqueued_temporaries_and_restart_preserves_the_shared_library()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let pool = finch_server::db::init_db("sqlite::memory:").await?;
    let mut config = ServerConfig::default();
    config.music.storage_dir = Some(dir.path().to_string_lossy().into());
    let service = Arc::new(MusicService::new(pool.clone(), &config));
    service.startup_cleanup().await?;
    let now = 5_000;
    let persistent = add(&service, false, false, 0).await?;
    let fresh = add(&service, true, false, now - 10).await?;
    let expired = add(&service, true, false, now - TEMP_LEASE_SECS).await?;
    let queued = add(&service, true, true, 0).await?;
    service.sweep(now).await?;
    assert!(service.record(expired.id).await.is_err());
    assert!(!service.file_path(expired.id, "mp3")?.exists());
    assert!(service.record(fresh.id).await.is_ok());
    assert!(service.record(queued.id).await.is_ok());
    let partial = dir.path().join(format!("{}.part", Uuid::new_v4()));
    tokio::fs::write(&partial, b"interrupted").await?;
    let orphan = dir.path().join(format!("{}.mp3", Uuid::new_v4()));
    tokio::fs::write(&orphan, b"ID3uncommitted").await?;
    service.startup_cleanup().await?;
    assert!(service.record(fresh.id).await.is_err());
    assert!(service.record(queued.id).await.is_err());
    assert!(!partial.exists());
    assert!(
        !orphan.exists(),
        "a crash between rename and database commit must not leak files"
    );
    assert_eq!(service.library().await?, vec![persistent.clone()]);
    assert_eq!(
        tokio::fs::read(service.file_path(persistent.id, "mp3")?).await?,
        b"ID3x"
    );
    Ok(())
}

#[tokio::test]
async fn migration_from_the_1_4_2_schema_preserves_existing_users_and_messages()
-> anyhow::Result<()> {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await?;
    let mut legacy = sqlx::migrate!("./migrations");
    legacy.migrations = Cow::Owned(
        legacy
            .migrations
            .iter()
            .filter(|migration| migration.version < 12)
            .cloned()
            .collect(),
    );
    legacy.run(&pool).await?;
    sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES ('u', 'alice', 'Alice', 'hash', 0)").execute(&pool).await?;
    sqlx::query("INSERT INTO channels (id, short_id, name, channel_type, created_at) VALUES ('c', 1, 'chat', 'text', 0)").execute(&pool).await?;
    sqlx::query("INSERT INTO messages (id, channel_id, author_id, content, created_at) VALUES ('m', 'c', 'u', 'keep this message', 0)").execute(&pool).await?;
    let users_before: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, username, password_hash FROM users")
            .fetch_all(&pool)
            .await?;
    let messages_before: Vec<(String, String)> = sqlx::query_as("SELECT id, content FROM messages")
        .fetch_all(&pool)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    let users_after: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, username, password_hash FROM users")
            .fetch_all(&pool)
            .await?;
    let messages_after: Vec<(String, String)> = sqlx::query_as("SELECT id, content FROM messages")
        .fetch_all(&pool)
        .await?;
    assert_eq!(users_after, users_before);
    assert_eq!(messages_after, messages_before);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_tracks")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    Ok(())
}
