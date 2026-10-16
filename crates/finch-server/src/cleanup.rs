//! Housekeeping: files that were uploaded but never attached to a message are removed after a day.

use std::time::Duration;

use sqlx::SqlitePool;
use tokio::task::JoinHandle;

/// An upload that is still not part of any message after this long is thrown away.
pub const ORPHAN_AGE_SECS: i64 = 24 * 3600;
const EVERY: Duration = Duration::from_secs(3600);

/// Remove unattached uploads older than `ORPHAN_AGE_SECS` (rows and files). Returns how many.
pub async fn remove_orphans(pool: &SqlitePool, now: i64) -> Result<usize, sqlx::Error> {
    // Claim expired uploads in one SQLite statement. Only rows actually deleted authorize
    // disk removal; a concurrent message linking an upload must keep its file intact.
    let rows = sqlx::query_as::<_, (String, Option<String>)>(
        "DELETE FROM attachments WHERE message_id IS NULL AND created_at < ? RETURNING storage_path, thumb_path",
    )
    .bind(now - ORPHAN_AGE_SECS)
    .fetch_all(pool)
    .await?;
    for (path, thumb) in &rows {
        let _ = std::fs::remove_file(path);
        if let Some(thumb) = thumb {
            let _ = std::fs::remove_file(thumb);
        }
    }
    Ok(rows.len())
}

/// Run `remove_orphans` now and then every hour, until the task is aborted.
pub fn spawn(pool: SqlitePool) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match remove_orphans(&pool, crate::api::invites::now_secs()).await {
                Ok(0) => {}
                Ok(count) => tracing::info!("Removed {count} unattached uploads"),
                Err(error) => tracing::warn!("Cleaning up uploads failed: {error}"),
            }
            tokio::time::sleep(EVERY).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_file_that_was_not_deleted_from_the_database_stays_on_disk() -> anyhow::Result<()> {
        let pool = crate::db::init_db("sqlite::memory:").await?;
        sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES ('u1', 'u', 'U', 'x', 0)").execute(&pool).await?;
        let channel: String = sqlx::query_scalar("SELECT id FROM channels LIMIT 1").fetch_one(&pool).await?;
        sqlx::query("INSERT INTO messages (id, channel_id, author_id, content, created_at) VALUES ('m1', ?, 'u1', 'hi', 0)").bind(&channel).execute(&pool).await?;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("claimed-upload");
        std::fs::write(&path, b"keep me")?;
        add(&pool, "a1", &path, None, 0).await;
        // Deterministically model an upload being claimed before the DELETE completes.
        // A skipped DELETE must never authorize removing the corresponding disk file.
        sqlx::query("CREATE TRIGGER claim_upload BEFORE DELETE ON attachments BEGIN UPDATE attachments SET message_id = 'm1' WHERE id = OLD.id; SELECT RAISE(IGNORE); END").execute(&pool).await?;
        assert_eq!(remove_orphans(&pool, ORPHAN_AGE_SECS + 1).await?, 0);
        assert_eq!(std::fs::read(&path)?, b"keep me");
        let message: String = sqlx::query_scalar("SELECT message_id FROM attachments WHERE id = 'a1'").fetch_one(&pool).await?;
        assert_eq!(message, "m1");
        Ok(())
    }

    async fn add(pool: &SqlitePool, id: &str, path: &std::path::Path, message: Option<&str>, created_at: i64) {
        sqlx::query("INSERT INTO attachments (id, uploader_id, message_id, filename, size_bytes, content_type, storage_path, created_at) VALUES (?, ?, ?, 'f', 1, 'text/plain', ?, ?)")
            .bind(id)
            .bind("u1")
            .bind(message)
            .bind(path.to_string_lossy().to_string())
            .bind(created_at)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn old_unattached_files_go_and_everything_else_stays() -> anyhow::Result<()> {
        let pool = crate::db::init_db("sqlite::memory:").await?;
        sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES ('u1', 'u', 'U', 'x', 0)").execute(&pool).await?;
        let channel: String = sqlx::query_scalar("SELECT id FROM channels LIMIT 1").fetch_one(&pool).await?;
        sqlx::query("INSERT INTO messages (id, channel_id, author_id, content, created_at) VALUES ('m1', ?, 'u1', 'hi', 0)").bind(&channel).execute(&pool).await?;

        let dir = tempfile::tempdir()?;
        let now = 1_000_000;
        let paths: Vec<_> = ["old-orphan", "fresh-orphan", "attached"].iter().map(|n| dir.path().join(n)).collect();
        for path in &paths {
            std::fs::write(path, b"x")?;
        }
        add(&pool, "a1", &paths[0], None, now - ORPHAN_AGE_SECS - 1).await;
        add(&pool, "a2", &paths[1], None, now - 60).await;
        add(&pool, "a3", &paths[2], Some("m1"), now - ORPHAN_AGE_SECS * 10).await;

        assert_eq!(remove_orphans(&pool, now).await?, 1);
        assert!(!paths[0].exists());
        assert!(paths[1].exists() && paths[2].exists());
        let left: Vec<String> = sqlx::query_scalar("SELECT id FROM attachments ORDER BY id").fetch_all(&pool).await?;
        assert_eq!(left, vec!["a2", "a3"]);
        assert_eq!(remove_orphans(&pool, now).await?, 0, "nothing more to do");
        Ok(())
    }
}
