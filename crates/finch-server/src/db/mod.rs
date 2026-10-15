//! Database connection pool setup, SQLite PRAGMAs, and migrations.

use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Initialize the SQLite database connection pool and run embedded migrations.
pub async fn init_db(database_url: &str) -> Result<SqlitePool> {
    let connection_options = SqliteConnectOptions::from_str(database_url)
        .context("Invalid database URL")?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_millis(5000))
        .foreign_keys(true)
        // 20 MB cache size (-20000 KiB)
        .pragma("cache_size", "-20000");

    let pool = SqlitePoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect_with(connection_options)
        .await
        .context("Failed to connect to SQLite database")?;

    // Run embedded migrations from ./migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run database migrations")?;

    tracing::info!("SQLite database initialized and migrations applied successfully");

    // Seed default channels if the table is empty
    seed_default_channels(&pool).await?;

    Ok(pool)
}

/// Seed default text and voice channels on a fresh server instance.
async fn seed_default_channels(pool: &SqlitePool) -> Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels")
        .fetch_one(pool)
        .await?;

    if count == 0 {
        let now = chrono::Utc::now().timestamp();

        // 1. Default text channel
        let text_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO channels (id, short_id, name, channel_type, position, topic, bitrate_kbps, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(text_id)
        .bind(1) // short_id 1
        .bind("general")
        .bind("text")
        .bind(0)
        .bind("Welcome to FinchSpeak!")
        .bind(64)
        .bind(now)
        .execute(pool)
        .await?;

        // 2. Default voice channel
        let voice_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO channels (id, short_id, name, channel_type, position, topic, bitrate_kbps, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(voice_id)
        .bind(2) // short_id 2
        .bind("General Voice")
        .bind("voice")
        .bind(1)
        .bind("Default voice room")
        .bind(64)
        .bind(now)
        .execute(pool)
        .await?;

        tracing::info!("Seeded default channels (#general and General Voice)");
    }

    Ok(())
}

// Minimal fallback for chrono::Utc::now().timestamp() using std::time::SystemTime
mod chrono {
    pub struct Utc;
    pub struct DateTime;
    impl Utc {
        pub fn now() -> DateTime { DateTime }
    }
    impl DateTime {
        pub fn timestamp(&self) -> i64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
        }
    }
}
