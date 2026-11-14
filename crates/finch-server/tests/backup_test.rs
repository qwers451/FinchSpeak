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

#[cfg(unix)]
fn mode_of(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Backups hold password hashes: the directory and every backup are owner-only from creation.
#[cfg(unix)]
#[tokio::test]
async fn backups_and_their_directory_are_owner_only() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let pool = database_with_a_user(dir.path()).await?;
    let backups = dir.path().join("backups");

    let backup = create_backup(&pool, &backups, 7).await?;
    assert_eq!(mode_of(&backup), 0o600);
    assert_eq!(mode_of(&backups), 0o700);
    Ok(())
}

/// Backups written by an earlier version are tightened at startup; other files are left alone.
#[cfg(unix)]
#[test]
fn startup_tightens_existing_backups_only() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let backups = dir.path().join("backups");
    std::fs::create_dir(&backups)?;
    std::fs::set_permissions(&backups, std::fs::Permissions::from_mode(0o755))?;
    let old = backups.join("finchspeak-000000000001-000.db");
    std::fs::write(&old, "x")?;
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o644))?;
    let notes = backups.join("notes.txt");
    std::fs::write(&notes, "not a backup")?;
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o644))?;

    finch_server::backup::tighten_backups(&backups);
    assert_eq!(mode_of(&backups), 0o700);
    assert_eq!(mode_of(&old), 0o600);
    assert_eq!(mode_of(&notes), 0o644);
    Ok(())
}
