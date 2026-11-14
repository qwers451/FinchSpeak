//! REST API router configuration.

pub mod admin;
pub mod auth;
pub mod avatars;
pub mod join_sounds;
pub mod channels;
pub mod files;
pub mod health;
pub mod invites;
pub mod messages;
pub mod music;
pub mod search;
pub mod soundboard;
pub mod state;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderName, HeaderValue, Method};
use axum::routing::{get, post};
use axum::Router;
use sqlx::SqlitePool;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::api::auth::ArcSharedState;
use crate::config::ServerConfig;
use crate::session::SessionManager;

/// Origins of the desktop app's webview (Tauri 2 on each platform). Always allowed.
const APP_ORIGINS: [&str; 3] = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];

/// Turn `[server] allowed_origins` entries into header values. An entry must be `http://`, `https://`
/// or `tauri://` followed by a host, with no path, query, user info, wildcard or whitespace; anything
/// else is skipped with a warning.
pub(crate) fn parse_allowed_origins(entries: &[String]) -> Vec<HeaderValue> {
    entries
        .iter()
        .filter_map(|entry| {
            let trimmed = entry.trim();
            let valid = trimmed.split_once("://").is_some_and(|(scheme, host)| {
                matches!(scheme, "http" | "https" | "tauri")
                    && !host.is_empty()
                    && !host.chars().any(|c| matches!(c, '/' | '?' | '#' | '@' | '*') || c.is_whitespace())
            });
            match HeaderValue::from_str(trimmed) {
                Ok(value) if valid => Some(value),
                _ => {
                    tracing::warn!("[server] allowed_origins: skipping invalid origin {entry:?}");
                    None
                }
            }
        })
        .collect()
}

/// CORS for the HTTP API: the desktop app's origins plus the configured ones. Only the methods and
/// headers the API uses are allowed. Requests without an Origin header (the native client) are not
/// affected by CORS.
pub(crate) fn cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins = APP_ORIGINS
        .iter()
        .map(|origin| HeaderValue::from_static(origin))
        .chain(parse_allowed_origins(allowed_origins));
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::IF_NONE_MATCH,
            HeaderName::from_static("x-finch-file-name"),
            HeaderName::from_static("x-finch-keep"),
        ])
}

/// Build the Axum router for the FinchSpeak REST API.
pub fn create_router(pool: SqlitePool, mut config: ServerConfig, sessions: SessionManager) -> Router {
    config.storage = config.storage.validated();
    let swept = files::sweep_stale_parts(std::path::Path::new(&config.storage.upload_dir));
    if swept > 0 {
        tracing::info!("Removed {swept} unfinished uploads");
    }
    // axum caps request bodies at 2 MB by default; uploads follow the configured limit instead.
    let upload_limit = files::upload_body_limit(config.storage.max_file_size_mb);
    let music_upload_limit = config.music.max_track_mb.saturating_mul(crate::music::MIB).saturating_add(64 * 1024).min(usize::MAX as u64) as usize;
    sessions.music.get_or_init(|| std::sync::Arc::new(crate::music::MusicService::new(pool.clone(), &config)));
    sessions.soundboard.get_or_init(|| config.soundboard.clone());
    health::mark_started();
    let monitor = crate::health::HealthMonitor::start(pool.clone(), &config, sessions.clone());
    let auth_hash_gate = std::sync::Arc::new(tokio::sync::Semaphore::new(config.limits.clone().validated().auth_hash_concurrency));
    auth::warm_dummy_hash();
    let cors = cors_layer(&config.server.allowed_origins);
    let shared_state = ArcSharedState { pool, config, sessions, login_guard: Default::default(), login_ip_hits: Default::default(), auth_hash_gate, register_hits: Default::default(), avatar_hits: Default::default() };

    Router::new()
        .route("/health", get(health::health_handler))
        // Auth routes
        .route("/api/auth/register", post(auth::register_handler))
        .route("/api/auth/login", post(auth::login_handler))
        .route("/api/auth/logout", post(auth::logout_handler))
        .route("/api/auth/refresh", post(auth::refresh_handler))
        .route("/api/auth/sessions", get(auth::list_sessions_handler))
        .route("/api/auth/sessions/revoke-others", post(auth::revoke_other_sessions_handler))
        .route("/api/auth/sessions/{id}", axum::routing::delete(auth::end_session_handler))
        .route("/api/auth/me", get(auth::me_handler).patch(auth::update_profile_handler))
        .route("/api/auth/password", post(auth::change_password_handler))
        .route("/api/auth/avatar", post(avatars::upload_handler).delete(avatars::delete_handler).layer(DefaultBodyLimit::max(4 * 1024 * 1024)))
        .route("/api/users/{id}/avatar", get(avatars::download_handler))
        .route("/api/auth/join-sound", post(join_sounds::upload_handler).delete(join_sounds::delete_handler).layer(DefaultBodyLimit::max(1024 * 1024)))
        .route("/api/users/{id}/join-sound", get(join_sounds::download_handler))
        .route("/api/state", get(state::state_handler))
        .route("/api/music/tracks", get(music::library).post(music::upload).layer(DefaultBodyLimit::max(music_upload_limit)))
        .route("/api/music/tracks/{id}/file", get(music::download))
        .route("/api/music/tracks/{id}", axum::routing::delete(music::delete))
        .route("/api/soundboard", get(soundboard::list_handler).post(soundboard::upload_handler).layer(DefaultBodyLimit::max(soundboard::MAX_SOUND_BYTES + 64 * 1024)))
        .route("/api/soundboard/{id}/file", get(soundboard::file_handler))
        .route("/api/soundboard/{id}", axum::routing::delete(soundboard::delete_handler))
        .route("/api/search", get(search::search_handler))
        // Moderation (admins)
        .route("/api/admin/users", get(admin::list_users_handler))
        .route("/api/admin/users/{id}/role", axum::routing::put(admin::set_role_handler))
        .route("/api/admin/users/{id}/kick", post(admin::kick_handler))
        .route("/api/admin/users/{id}/ban", post(admin::ban_handler))
        .route("/api/admin/users/{id}/unban", post(admin::unban_handler))
        .route("/api/admin/users/{id}/password-reset", post(admin::reset_password_handler))
        .route("/api/admin/users/{id}/voice", post(admin::moderate_voice_handler))
        .route("/api/admin/audit", get(admin::audit_handler))
        .route("/api/admin/server", get(admin::overview_handler))
        .route("/api/admin/health", get(admin::health_handler).layer(axum::Extension(monitor)))
        .route("/api/admin/registration", axum::routing::put(admin::set_registration_handler))
        .route("/api/admin/backup", post(admin::backup_now_handler))
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
        .route("/api/channels/{channel_id}/read", post(channels::mark_read_handler))
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
        .route("/api/files/{id}/thumb", get(files::download_thumb_handler))
        .layer(cors)
        .with_state(shared_state)
}

#[cfg(test)]
mod tests {
    use super::parse_allowed_origins;

    #[test]
    fn configured_origins_must_be_scheme_and_host_only() {
        let entries = [
            "https://finch.example.com".to_string(),
            "  http://192.168.1.20:8443 ".to_string(),
            "tauri://localhost".to_string(),
            "https://finch.example.com/".to_string(),
            "https://finch.example.com/path".to_string(),
            "*".to_string(),
            "https://*.example.com".to_string(),
            "ftp://files.example.com".to_string(),
            "not an origin".to_string(),
            "https://".to_string(),
            "https://user@finch.example.com".to_string(),
            "".to_string(),
        ];
        let kept: Vec<String> = parse_allowed_origins(&entries)
            .into_iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect();
        assert_eq!(
            kept,
            vec![
                "https://finch.example.com".to_string(),
                "http://192.168.1.20:8443".to_string(),
                "tauri://localhost".to_string(),
            ]
        );
    }
}
