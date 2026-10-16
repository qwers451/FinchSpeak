//! REST API router configuration.

pub mod admin;
pub mod auth;
pub mod channels;
pub mod files;
pub mod invites;
pub mod messages;
pub mod state;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use sqlx::SqlitePool;
use tower_http::cors::{Any, CorsLayer};

use crate::api::auth::ArcSharedState;
use crate::config::ServerConfig;
use crate::session::SessionManager;

/// Build the Axum router for the FinchSpeak REST API.
pub fn create_router(pool: SqlitePool, config: ServerConfig, sessions: SessionManager) -> Router {
    // axum caps request bodies at 2 MB by default; uploads follow the configured limit instead.
    let upload_limit = files::upload_body_limit(config.storage.max_file_size_mb);
    let shared_state = ArcSharedState { pool, config, sessions };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Auth routes
        .route("/api/auth/register", post(auth::register_handler))
        .route("/api/auth/login", post(auth::login_handler))
        .route("/api/auth/logout", post(auth::logout_handler))
        .route("/api/auth/me", get(auth::me_handler).patch(auth::update_profile_handler))
        .route("/api/auth/password", post(auth::change_password_handler))
        .route("/api/state", get(state::state_handler))
        // Moderation (admins)
        .route("/api/admin/users", get(admin::list_users_handler))
        .route("/api/admin/users/{id}/role", axum::routing::put(admin::set_role_handler))
        .route("/api/admin/users/{id}/kick", post(admin::kick_handler))
        .route("/api/admin/users/{id}/ban", post(admin::ban_handler))
        .route("/api/admin/users/{id}/unban", post(admin::unban_handler))
        // Invitations
        .route("/api/invites", post(invites::create_invite_handler).get(invites::list_invites_handler))
        .route("/api/invites/{code}", axum::routing::delete(invites::revoke_invite_handler))
        // Channels routes
        .route("/api/channels", get(channels::list_channels_handler))
        .route("/api/channels", post(channels::create_channel_handler))
        .route("/api/channels/order", axum::routing::put(channels::reorder_channels_handler))
        .route(
            "/api/channels/{id}",
            axum::routing::patch(channels::update_channel_handler).delete(channels::delete_channel_handler),
        )
        // Editing and deleting messages
        .route(
            "/api/messages/{id}",
            axum::routing::patch(messages::edit_message_handler).delete(messages::delete_message_handler),
        )
        .route(
            "/api/messages/{id}/reactions/{emoji}",
            axum::routing::put(messages::add_reaction_handler).delete(messages::remove_reaction_handler),
        )
        // Messages history routes
        .route(
            "/api/channels/{channel_id}/messages",
            get(messages::channel_messages_handler),
        )
        // File attachments routes
        .route(
            "/api/files/upload",
            post(files::upload_file_handler).layer(DefaultBodyLimit::max(upload_limit)),
        )
        .route("/api/files/{id}", get(files::download_file_handler))
        .layer(cors)
        .with_state(shared_state)
}
