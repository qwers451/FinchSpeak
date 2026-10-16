//! `/health`: an unauthenticated liveness probe for Docker, monitoring and the load test.
use std::sync::OnceLock;
use std::time::Instant;

use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::api::auth::ArcSharedState;

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Remember when the process started; called while the router is built.
pub fn mark_started() {
    STARTED.get_or_init(Instant::now);
}

/// Seconds since the router was built.
pub fn uptime_secs() -> u64 {
    STARTED.get().map_or(0, |started| started.elapsed().as_secs())
}

/// Reports `ok` only when the database answers. Nothing about users or channels is exposed.
pub async fn health_handler(State(state): State<ArcSharedState>) -> (StatusCode, Json<Value>) {
    let database_ok = sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&state.pool).await.is_ok();
    let uptime = STARTED.get().map_or(0, |started| started.elapsed().as_secs());
    let status = if database_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (
        status,
        Json(json!({
            "status": if database_ok { "ok" } else { "degraded" },
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_secs": uptime,
        })),
    )
}
