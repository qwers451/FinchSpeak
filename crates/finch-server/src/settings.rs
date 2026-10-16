//! Settings that an admin changes while the server runs (stored in the database, not in the TOML).

use sqlx::SqlitePool;

use crate::config::RegistrationMode;

pub async fn get(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(pool).await.ok().flatten()
}

pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn registration_text(mode: RegistrationMode) -> &'static str {
    match mode {
        RegistrationMode::Open => "open",
        RegistrationMode::Invite => "invite",
    }
}

/// The registration mode in force: the one an admin chose in the app, else the one in the config file.
pub async fn registration_mode(pool: &SqlitePool, from_config: RegistrationMode) -> RegistrationMode {
    match get(pool, "registration").await.as_deref() {
        Some("open") => RegistrationMode::Open,
        Some("invite") => RegistrationMode::Invite,
        _ => from_config,
    }
}
