//! Database backups: a restorable copy of the live data, and only the newest are kept.

use finch_server::backup::{create_backup, prune};
use finch_server::db::init_db;

async fn database_with_a_user(dir: &std::path::Path) -> anyhow::Result<sqlx::SqlitePool> {
    let url = format!("sqlite://{}?mode=rwc", dir.join("live.db").to_string_lossy().replace('\\', "/"));
    let pool = init_db(&url).await?;
    sqlx::query("INSERT INTO users (id, username, display_name, password_hash, role, status, created_at) VALUES ('u1', 'alice', 'Alice', 'x', 'member', 'offline', 1)")
        .execute(&pool)
        .await?;
    Ok(pool)
}

#[tokio::test]
async fn a_backup_is_a_database_with_the_same_data() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let pool = database_with_a_user(dir.path()).await?;

    let backup = create_backup(&pool, &dir.path().join("backups"), 7).await?;
    assert!(backup.exists());

    // Open the copy on its own and find the data.
    let copy = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=ro", backup.to_string_lossy().replace('\\', "/"))).await?;
    let name: String = sqlx::query_scalar("SELECT username FROM users WHERE id = 'u1'").fetch_one(&copy).await?;
    assert_eq!(name, "alice");

    // Later changes to the live database do not reach the copy.
    sqlx::query("DELETE FROM users").execute(&pool).await?;
    let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&copy).await?;
    assert_eq!(still, 1);
    Ok(())
}

#[tokio::test]
async fn only_the_newest_backups_are_kept() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let pool = database_with_a_user(dir.path()).await?;
    let backups = dir.path().join("backups");

    let mut written = Vec::new();
    for _ in 0..5 {
        written.push(create_backup(&pool, &backups, 3).await?);
    }
    let mut left: Vec<_> = std::fs::read_dir(&backups)?.map(|e| e.map(|e| e.path())).collect::<Result<_, _>>()?;
    left.sort();
    assert_eq!(left.len(), 3);
    assert_eq!(left, written[2..].to_vec(), "the three newest remain");
    Ok(())
}

#[tokio::test]
async fn pruning_ignores_files_that_are_not_backups() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.txt"), "keep me")?;
    for n in 0..4 {
        std::fs::write(dir.path().join(format!("finchspeak-{n:012}-000.db")), "x")?;
    }
    prune(dir.path(), 2)?;
    assert!(dir.path().join("notes.txt").exists());
    assert!(!dir.path().join("finchspeak-000000000000-000.db").exists());
    assert!(dir.path().join("finchspeak-000000000003-000.db").exists());
    Ok(())
}
