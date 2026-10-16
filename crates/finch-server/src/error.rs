//! Server error types and HTTP response formatting.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Unified server error type.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("User already exists")]
    UserAlreadyExists,

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Forbidden")]
    Forbidden,

    #[error("This account is banned")]
    Banned,

    #[error("A valid invite code is required")]
    InviteRequired,

    #[error("File upload error: {0}")]
    FileUpload(String),

    #[error("Too many requests, retry in {0} s")]
    TooManyRequests(u64),

    #[error("Internal server error: {0}")]
    Internal(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            Self::AuthFailed(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Self::UserAlreadyExists => (StatusCode::CONFLICT, "Username is already taken".to_string()),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Invalid or missing token".to_string()),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Permission denied".to_string()),
            Self::Banned => (StatusCode::FORBIDDEN, "This account has been banned from the server".to_string()),
            Self::InviteRequired => (StatusCode::FORBIDDEN, "A valid invite code is required to register".to_string()),
            Self::FileUpload(msg) => (StatusCode::PAYLOAD_TOO_LARGE, msg.clone()),
            Self::TooManyRequests(wait) => (StatusCode::TOO_MANY_REQUESTS, format!("Too many attempts. Try again in {wait} seconds")),
            Self::Database(err) => {
                tracing::error!("Database query failed: {err}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error occurred".to_string())
            }
            Self::Internal(err) => {
                tracing::error!("Internal error: {err}");
                (StatusCode::INTERNAL_SERVER_ERROR, "An internal server error occurred".to_string())
            }
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}
