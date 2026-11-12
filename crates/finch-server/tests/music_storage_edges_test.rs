use finch_server::{
    config::ServerConfig,
    music::{MusicService, UploadGuard},
};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn reservations_include_inflight_bytes_and_release_after_cancelled_upload()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let pool = finch_server::db::init_db("sqlite::memory:").await?;
    let mut config = ServerConfig::default();
    config.music.storage_dir = Some(dir.path().to_string_lossy().into());
    config.music.temp_quota_mb_per_user = 1;
    config.music.temp_quota_mb_total = 2;
    let service = Arc::new(MusicService::new(pool, &config));
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();
    let guard = UploadGuard::new(service.clone());
    service.reserve(guard.id, alice, false, 1024 * 1024).await?;
    tokio::fs::write(&guard.partial, b"pending").await?;
    assert!(
        service
            .reserve(Uuid::new_v4(), alice, false, 1)
            .await
            .is_err()
    );
    let second = Uuid::new_v4();
    service.reserve(second, bob, false, 1024 * 1024).await?;
    assert!(
        service
            .reserve(Uuid::new_v4(), Uuid::new_v4(), false, 1)
            .await
            .is_err()
    );
    let partial = guard.partial.clone();
    drop(guard);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if !partial.exists()
                && service
                    .reserve(Uuid::nil(), alice, false, 1024 * 1024)
                    .await
                    .is_ok()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    service.release_reservation(second);
    service.release_reservation(Uuid::nil());
    Ok(())
}
