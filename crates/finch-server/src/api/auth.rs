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
    pub session_id: String,
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
        let claims = verify_active_jwt(token, &shared.config.auth.jwt_secret, &shared.pool).await?;

        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ServerError::Unauthorized)?;

        Ok(AuthUser {
            session_id: claims.jti,
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

/// A signed JWT is usable only while its server-side session and account still exist.
pub async fn verify_active_jwt(token: &str, secret: &str, pool: &SqlitePool) -> Result<Claims, ServerError> {
    let mut claims = verify_jwt(token, secret)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs() as i64;
    let account = sqlx::query_as::<_, (String, String)>(
        "SELECT u.username, u.role FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id = ? AND s.user_id = ? AND s.expires_at > ?",
    ).bind(&claims.jti).bind(&claims.sub).bind(now).fetch_optional(pool).await?
        .ok_or(ServerError::Unauthorized)?;
    claims.username = account.0;
    claims.role = account.1;
    Ok(claims)
}

pub async fn logout_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<StatusCode, ServerError> {
    sqlx::query("DELETE FROM sessions WHERE id = ? AND user_id = ?")
        .bind(&auth.session_id).bind(auth.user_id.to_string()).execute(&state.pool).await?;
    let connections: Vec<_> = state.sessions.sessions.iter()
        .filter(|session| session.token_id == auth.session_id)
        .map(|session| session.connection.clone()).collect();
    for connection in connections { connection.close(0u32.into(), b"token revoked"); }
    Ok(StatusCode::NO_CONTENT)
}

// ─── API DTOs & Handlers ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    /// Required when the server only registers people with an invitation.
    pub invite_code: Option<String>,
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
    /// UDP port of the QUIC server, so clients do not have to guess it.
    pub quic_port: u16,
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

    // One transaction: spending the invite and creating the account stand or fall together.
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    if state.config.auth.registration == crate::config::RegistrationMode::Invite {
        let first_user = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users").fetch_one(&mut *tx).await? == 0;
        if !first_user {
            let code = payload.invite_code.as_deref().map(|c| c.trim().to_uppercase()).unwrap_or_default();
            let spent = sqlx::query(
                "UPDATE invites SET uses = uses + 1 WHERE code = ? AND uses < max_uses AND expires_at > ?",
            )
            .bind(&code)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if spent.rows_affected() != 1 {
                return Err(ServerError::InviteRequired);
            }
        }
    }

    // SQLite serializes the write and the role decision as one statement.
    let res = sqlx::query_scalar::<_, String>(
        "INSERT INTO users (id, username, display_name, password_hash, role, status, created_at)
         VALUES (?, ?, ?, ?, CASE WHEN NOT EXISTS (SELECT 1 FROM users) THEN 'admin' ELSE 'member' END, 'offline', ?)
         RETURNING role"
    )
    .bind(user_id.to_string())
    .bind(&username)
    .bind(&display_name)
    .bind(&password_hash)
    .bind(now)
    .fetch_one(&mut *tx)
    .await;

    match res {
        Ok(role) => {
            tx.commit().await?;
            let (token, jti) = issue_jwt(
                &user_id,
                &username,
                &role,
                &state.config.auth.jwt_secret,
                state.config.auth.jwt_expiration_hours,
            )?;

            // Store session
            let exp = now + (state.config.auth.jwt_expiration_hours * 3600);
            sqlx::query("INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?, ?, ?, ?)")
                .bind(jti)
                .bind(user_id.to_string())
                .bind(exp)
                .bind(now)
                .execute(&state.pool)
                .await?;

            let response = AuthResponse {
                quic_port: state.config.server.quic_port,
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

    let row = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<i64>)>(
        "SELECT id, password_hash, display_name, role, avatar_path, banned_at FROM users WHERE username = ?"
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await?;

    let (user_id_str, hash, display_name, role, avatar_path, banned_at) = match row {
        Some(r) => r,
        None => return Err(ServerError::AuthFailed("Invalid username or password".to_string())),
    };

    if !verify_password(&payload.password, &hash) {
        return Err(ServerError::AuthFailed("Invalid username or password".to_string()));
    }

    if banned_at.is_some() {
        return Err(ServerError::Banned);
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

    sqlx::query("INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?, ?, ?, ?)")
        .bind(jti)
        .bind(&user_id_str)
        .bind(exp)
        .bind(now)
        .execute(&state.pool)
        .await?;

    let response = AuthResponse {
        quic_port: state.config.server.quic_port,
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

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// PATCH /api/auth/me: change your display name.
pub async fn update_profile_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Json(request): Json<UpdateProfileRequest>,
) -> Result<impl IntoResponse, ServerError> {
    let name = request.display_name.trim().to_string();
    if name.is_empty() || name.chars().count() > 32 {
        return Err(ServerError::BadRequest("Display name must be between 1 and 32 characters".into()));
    }
    sqlx::query("UPDATE users SET display_name = ? WHERE id = ?")
        .bind(&name)
        .bind(auth.user_id.to_string())
        .execute(&state.pool)
        .await?;
    // Live sessions show the new name at once.
    for mut session in state.sessions.sessions.iter_mut() {
        if session.user_id == auth.user_id {
            session.username = name.clone();
        }
    }
    state
        .sessions
        .broadcast_all(finch_protocol::messages::ServerMessage::UserProfileChanged {
            user_id: auth.user_id,
            display_name: name,
        })
        .await;
    me_handler(auth, State(state)).await
}

/// POST /api/auth/password: needs the current password; signs out every other device.
pub async fn change_password_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<StatusCode, ServerError> {
    if request.new_password.len() < 6 {
        return Err(ServerError::BadRequest("Password must be at least 6 characters".into()));
    }
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(auth.user_id.to_string())
        .fetch_one(&state.pool)
        .await?;
    if !verify_password(&request.current_password, &hash) {
        return Err(ServerError::Forbidden);
    }
    let new_hash = hash_password(&request.new_password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(new_hash)
        .bind(auth.user_id.to_string())
        .execute(&state.pool)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id != ?")
        .bind(auth.user_id.to_string())
        .bind(&auth.session_id)
        .execute(&state.pool)
        .await?;
    let others: Vec<_> = state
        .sessions
        .sessions
        .iter()
        .filter(|session| session.user_id == auth.user_id && session.token_id != auth.session_id)
        .map(|session| session.connection.clone())
        .collect();
    for connection in others {
        connection.close(0u32.into(), b"password changed");
    }
    Ok(StatusCode::NO_CONTENT)
}
