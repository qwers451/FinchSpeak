//! Database backups: a consistent copy of the live database every few hours, oldest removed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tokio::task::JoinHandle;

use crate::config::BackupConfig;

const PREFIX: &str = "finchspeak-";
const SUFFIX: &str = ".db";
/// Wait a little after start so a crash loop does not fill the disk with copies.
const FIRST_BACKUP_DELAY: Duration = Duration::from_secs(60);

/// Write a backup into `dir` (created if needed) and delete all but the newest `keep`.
pub async fn create_backup(pool: &SqlitePool, dir: &Path, keep: usize) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create backup directory {dir:?}"))?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    // Name = time + counter, so sorting the names sorts the backups by age even within a second.
    // The counter continues after the highest one already present (older ones may be pruned).
    let same_second = format!("{PREFIX}{secs:012}-");
    let counter = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.strip_prefix(&same_second)?.strip_suffix(SUFFIX)?.parse::<u32>().ok()
        })
        .max()
        .map_or(0, |highest| highest + 1);
    let target = dir.join(format!("{same_second}{counter:03}{SUFFIX}"));
    sqlx::query("VACUUM INTO ?")
        .bind(target.to_string_lossy().to_string())
        .execute(pool)
        .await
        .with_context(|| format!("backup to {target:?} failed"))?;
    prune(dir, keep)?;
    Ok(target)
}

/// Keep the newest `keep` backups in `dir`.
pub fn prune(dir: &Path, keep: usize) -> Result<()> {
    let mut backups: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(SUFFIX))
        })
        .collect();
    backups.sort();
    let surplus = backups.len().saturating_sub(keep.max(1));
    for old in backups.into_iter().take(surplus) {
        std::fs::remove_file(&old).with_context(|| format!("cannot remove old backup {old:?}"))?;
    }
    Ok(())
}

/// Back up now-ish and then every `interval_hours`, until the task is aborted.
pub fn spawn(pool: SqlitePool, config: BackupConfig) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_BACKUP_DELAY).await;
        let interval = Duration::from_secs(config.interval_hours.max(1) * 3600);
        loop {
            match create_backup(&pool, Path::new(&config.dir), config.keep).await {
                Ok(path) => tracing::info!("Database backup written to {path:?}"),
                Err(e) => tracing::error!("Database backup failed: {e:#}"),
            }
            tokio::time::sleep(interval).await;
        }
    })
}
