//! User authentication, Argon2id password hashing, and JWT tokens.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::config::ServerConfig;
use crate::session::SessionManager;
use crate::error::ServerError;

/// JWT Claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject: user UUID.
    pub sub: String,
    pub username: String,
    pub role: String,
    /// Expiration timestamp in seconds.
    pub exp: usize,
    /// Unique token ID for session tracking.
    pub jti: String,
}

/// Extractor for authenticated routes.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub username: String,
    pub role: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    ArcSharedState: FromRef<S>,
{
    type Rejection = ServerError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let shared: ArcSharedState = FromRef::from_ref(state);
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or(ServerError::Unauthorized)?;

        if !auth_header.starts_with("Bearer ") {
            return Err(ServerError::Unauthorized);
        }

        let token = &auth_header["Bearer ".len()..];
        let claims = verify_jwt(token, &shared.config.auth.jwt_secret)?;

        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ServerError::Unauthorized)?;

        Ok(AuthUser {
            user_id,
            username: claims.username,
            role: claims.role,
        })
    }
}

// Marker helper for state extraction
pub trait FromRef<T> {
    fn from_ref(input: &T) -> Self;
}

#[derive(Clone)]
pub struct ArcSharedState {
    pub pool: SqlitePool,
    pub config: ServerConfig,
    /// Live QUIC sessions, shared with the QUIC server.
    pub sessions: SessionManager,
}

impl FromRef<ArcSharedState> for ArcSharedState {
    fn from_ref(input: &ArcSharedState) -> Self {
        input.clone()
    }
}

/// Hash a password using Argon2id.
pub fn hash_password(password: &str) -> Result<String, ServerError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| ServerError::Internal(format!("Failed to hash password: {e}")))?;
    Ok(hash.to_string())
}

/// Verify a password against an Argon2id hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    if let Ok(parsed_hash) = PasswordHash::new(hash) {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    } else {
        false
    }
}

/// Issue a new JWT token and session ID.
pub fn issue_jwt(
    user_id: &Uuid,
    username: &str,
    role: &str,
    secret: &str,
    expiration_hours: i64,
) -> Result<(String, String), ServerError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let exp = now + (expiration_hours as u64 * 3600);
    let jti = Uuid::new_v4().to_string();

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        exp: exp as usize,
        jti: jti.clone(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ServerError::Internal(format!("Failed to issue JWT: {e}")))?;

    Ok((token, jti))
}

/// Verify and decode a JWT token.
pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, ServerError> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| ServerError::Unauthorized)?;

    Ok(token_data.claims)
}

// ─── API DTOs & Handlers ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserDto,
}

#[derive(Debug, Serialize)]
pub struct UserDto {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub avatar_url: Option<String>,
}

/// Handler for POST /api/auth/register
pub async fn register_handler(
    State(state): State<ArcSharedState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<impl IntoResponse, ServerError> {
    let username = payload.username.trim().to_lowercase();
    if username.len() < 3 || username.len() > 32 {
        return Err(ServerError::BadRequest(
            "Username must be between 3 and 32 characters".to_string(),
        ));
    }
    if payload.password.len() < 6 {
        return Err(ServerError::BadRequest(
            "Password must be at least 6 characters".to_string(),
        ));
    }

    let display_name = payload
        .display_name
        .unwrap_or_else(|| username.clone());

    let password_hash = hash_password(&payload.password)?;
    let user_id = Uuid::new_v4();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Check if first user, make admin
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await?;
    let role = if user_count == 0 { "admin" } else { "member" };

    let res = sqlx::query(
        "INSERT INTO users (id, username, display_name, password_hash, role, status, created_at)
         VALUES (?, ?, ?, ?, ?, 'offline', ?)"
    )
    .bind(user_id.to_string())
    .bind(&username)
    .bind(&display_name)
    .bind(&password_hash)
    .bind(role)
    .bind(now)
    .execute(&state.pool)
    .await;

    match res {
        Ok(_) => {
            let (token, jti) = issue_jwt(
                &user_id,
                &username,
                role,
                &state.config.auth.jwt_secret,
                state.config.auth.jwt_expiration_hours,
            )?;

            // Store session
            let exp = now + (state.config.auth.jwt_expiration_hours * 3600);
            let _ = sqlx::query("INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?, ?, ?, ?)")
                .bind(jti)
                .bind(user_id.to_string())
                .bind(exp)
                .bind(now)
                .execute(&state.pool)
                .await;

            let response = AuthResponse {
                token,
                user: UserDto {
                    id: user_id.to_string(),
                    username,
                    display_name,
                    role: role.to_string(),
                    avatar_url: None,
                },
            };

            Ok((StatusCode::CREATED, Json(response)))
        }
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(ServerError::UserAlreadyExists)
        }
        Err(e) => Err(ServerError::Database(e)),
    }
}

/// Handler for POST /api/auth/login
pub async fn login_handler(
    State(state): State<ArcSharedState>,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, ServerError> {
    let username = payload.username.trim().to_lowercase();

    let row = sqlx::query_as::<_, (String, String, String, String, Option<String>)>(
        "SELECT id, password_hash, display_name, role, avatar_path FROM users WHERE username = ?"
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await?;

    let (user_id_str, hash, display_name, role, avatar_path) = match row {
        Some(r) => r,
        None => return Err(ServerError::AuthFailed("Invalid username or password".to_string())),
    };

    if !verify_password(&payload.password, &hash) {
        return Err(ServerError::AuthFailed("Invalid username or password".to_string()));
    }

    let user_id = Uuid::parse_str(&user_id_str).map_err(|_| ServerError::Unauthorized)?;

    let (token, jti) = issue_jwt(
        &user_id,
        &username,
        &role,
        &state.config.auth.jwt_secret,
        state.config.auth.jwt_expiration_hours,
    )?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let exp = now + (state.config.auth.jwt_expiration_hours * 3600);

    let _ = sqlx::query("INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?, ?, ?, ?)")
        .bind(jti)
        .bind(&user_id_str)
        .bind(exp)
        .bind(now)
        .execute(&state.pool)
        .await;

    let response = AuthResponse {
        token,
        user: UserDto {
            id: user_id_str,
            username,
            display_name,
            role,
            avatar_url: avatar_path,
        },
    };

    Ok(Json(response))
}

/// Handler for GET /api/auth/me
pub async fn me_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
) -> Result<impl IntoResponse, ServerError> {
    let row = sqlx::query_as::<_, (String, String, String, String, Option<String>)>(
        "SELECT id, username, display_name, role, avatar_path FROM users WHERE id = ?"
    )
    .bind(auth.user_id.to_string())
    .fetch_optional(&state.pool)
    .await?;

    let (id, username, display_name, role, avatar_path) = match row {
        Some(r) => r,
        None => return Err(ServerError::NotFound("User not found".to_string())),
    };

    let user = UserDto {
        id,
        username,
        display_name,
        role,
        avatar_url: avatar_path,
    };

    Ok(Json(user))
}
