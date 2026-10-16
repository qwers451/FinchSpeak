//! The log of administrative actions.

use sqlx::SqlitePool;

use crate::api::auth::AuthUser;

/// Entries older than this are removed.
pub const RETENTION_DAYS: i64 = 90;

/// Write one entry; a failure to write is logged but never blocks the action itself.
pub async fn record(pool: &SqlitePool, actor: &AuthUser, action: &str, target: Option<&str>, details: Option<&str>) {
    let now = crate::api::invites::now_secs();
    let result = sqlx::query("INSERT INTO audit_log (at, actor_id, actor_name, action, target, details) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(now)
        .bind(actor.user_id.to_string())
        .bind(&actor.username)
        .bind(action)
        .bind(target)
        .bind(details)
        .execute(pool)
        .await;
    if let Err(error) = result {
        tracing::error!("Could not write the audit log ({action}): {error}");
        return;
    }
    let _ = sqlx::query("DELETE FROM audit_log WHERE at < ?").bind(now - RETENTION_DAYS * 86_400).execute(pool).await;
}
