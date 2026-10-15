//! REST API router configuration.

pub mod auth;
pub mod channels;
pub mod files;
pub mod messages;

use axum::routing::{get, post};
use axum::Router;
use sqlx::SqlitePool;
use tower_http::cors::{Any, CorsLayer};

use crate::api::auth::ArcSharedState;
use crate::config::ServerConfig;
use crate::session::SessionManager;

/// Build the Axum router for the FinchSpeak REST API.
pub fn create_router(pool: SqlitePool, config: ServerConfig, sessions: SessionManager) -> Router {
    let shared_state = ArcSharedState { pool, config, sessions };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Auth routes
        .route("/api/auth/register", post(auth::register_handler))
        .route("/api/auth/login", post(auth::login_handler))
        .route("/api/auth/me", get(auth::me_handler))
        // Channels routes
        .route("/api/channels", get(channels::list_channels_handler))
        .route("/api/channels", post(channels::create_channel_handler))
        // Messages history routes
        .route(
            "/api/channels/{channel_id}/messages",
            get(messages::channel_messages_handler),
        )
        // File attachments routes
        .route("/api/files/upload", post(files::upload_file_handler))
        .route("/api/files/{id}", get(files::download_file_handler))
        .layer(cors)
        .with_state(shared_state)
}
