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
use std::sync::Arc;
use tokio::sync::Semaphore;
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
    pub login_guard: std::sync::Arc<crate::limits::LoginGuard>,
    /// Every sign-in attempt from one address, whether or not it is right.
    pub login_ip_hits: std::sync::Arc<crate::limits::WindowLimiter>,
    /// Bounds how many Argon2 hashes or checks run at once (`[limits] auth_hash_concurrency`).
    pub auth_hash_gate: Arc<Semaphore>,
    pub register_hits: std::sync::Arc<crate::limits::WindowLimiter>,
    pub avatar_hits: std::sync::Arc<crate::limits::WindowLimiter>,
}

impl FromRef<ArcSharedState> for ArcSharedState {
    fn from_ref(input: &ArcSharedState) -> Self {
        input.clone()
    }
}

/// Shortest password accepted when registering or choosing a new one (older, shorter ones still log in).
pub const MIN_PASSWORD_LEN: usize = 8;

/// Sign-in attempts (right or wrong) allowed from one address per window, on top of the per login and address lock.
pub const LOGIN_IP_LIMIT: usize = 20;
/// The window the per-address sign-in limit counts in.
pub const LOGIN_IP_WINDOW: std::time::Duration = std::time::Duration::from_secs(600);

/// Counts every sign-in call made by the test build, so tests can see that a hash check ran.
#[cfg(test)]
static VERIFY_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Runs one Argon2 job on the blocking pool, holding one of the `gate` permits while it runs, so the
/// number of hashes in memory at once is bounded. Waiting for a permit is normal, not a refusal.
/// A panicked or cancelled job is a 500, never a panic here.
///
/// The permit moves into the blocking job and is released only when the job ends. If the caller
/// stops waiting (its request was dropped), the job keeps running and keeps its permit.
async fn run_gated<T: Send + 'static>(gate: &Arc<Semaphore>, job: impl FnOnce() -> T + Send + 'static) -> Result<T, ServerError> {
    let permit = gate
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ServerError::Internal("password gate is closed".to_string()))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        job()
    })
    .await
    .map_err(|e| ServerError::Internal(format!("password task did not finish: {e}")))
}

/// Hash a password using Argon2id. Runs on the blocking pool, within the gate.
pub async fn hash_password(gate: &Arc<Semaphore>, password: String) -> Result<String, ServerError> {
    run_gated(gate, move || hash_password_blocking(&password)).await?
}

/// Verify a password against an Argon2id hash. Runs on the blocking pool, within the gate.
pub async fn verify_password(gate: &Arc<Semaphore>, password: String, hash: String) -> Result<bool, ServerError> {
    run_gated(gate, move || verify_password_blocking(&password, &hash)).await
}

/// Check a password against the dummy hash, for a login that does not exist. The dummy hash is
/// first made inside the blocking job, so no Argon2 work ever runs on an async worker.
async fn verify_unknown_login(gate: &Arc<Semaphore>, password: String) -> Result<(), ServerError> {
    run_gated(gate, move || -> Result<(), ServerError> {
        let hash = dummy_hash()?;
        verify_password_blocking(&password, hash);
        Ok(())
    })
    .await?
}

/// Makes the dummy hash in the background at startup, so the first unknown login is not slower.
/// Without a runtime nothing is started; the hash is then made on first use, inside a blocking job.
pub fn warm_dummy_hash() {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        drop(handle.spawn_blocking(|| {
            if let Err(error) = dummy_hash() {
                tracing::error!("could not prepare the dummy password hash: {error}");
            }
        }));
    }
}

fn hash_password_blocking(password: &str) -> Result<String, ServerError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| ServerError::Internal(format!("Failed to hash password: {e}")))?;
    Ok(hash.to_string())
}

fn verify_password_blocking(password: &str, hash: &str) -> bool {
    #[cfg(test)]
    VERIFY_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Ok(parsed_hash) = PasswordHash::new(hash) {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    } else {
        false
    }
}

/// A hash of a random-looking password, made with the same parameters as real ones. Unknown logins
/// are checked against it so their response time does not reveal whether the account exists.
fn dummy_hash() -> Result<&'static str, ServerError> {
    static DUMMY: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
    match DUMMY.get_or_init(|| hash_password_blocking("finchspeak-no-such-account").map_err(|e| e.to_string())) {
        Ok(hash) => Ok(hash.as_str()),
        Err(message) => Err(ServerError::Internal(message.clone())),
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
    let jti = Uuid::new_v4().to_string();
    let token = issue_jwt_for_session(user_id, username, role, secret, expiration_hours, &jti)?;
    Ok((token, jti))
}

/// Issue a JWT for an existing session id (used to refresh a session in place).
pub fn issue_jwt_for_session(
    user_id: &Uuid,
    username: &str,
    role: &str,
    secret: &str,
    expiration_hours: i64,
    jti: &str,
) -> Result<String, ServerError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let exp = (now as i64).saturating_add(expiration_hours.saturating_mul(3600)).max(0) as u64;

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        exp: exp as usize,
        jti: jti.to_string(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ServerError::Internal(format!("Failed to issue JWT: {e}")))
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

/// Why a token cannot be used any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenProblem {
    /// The token or its server-side session ran out of time.
    Expired,
    /// The signature is fine but the session is gone (logout, admin, password change).
    Revoked,
    /// Not a token this server signed.
    Invalid,
    /// The check itself failed (database trouble); says nothing about the token.
    Internal,
}

impl TokenProblem {
    /// The reason text sent to clients; the app tells the cases apart by it.
    pub fn message(self) -> &'static str {
        match self {
            TokenProblem::Expired => "Session expired",
            TokenProblem::Revoked => "Session revoked",
            TokenProblem::Invalid => "Invalid token",
            TokenProblem::Internal => "Server error, try again",
        }
    }
}

/// A signed JWT is usable only while its server-side session and account still exist.
pub async fn check_active_jwt(token: &str, secret: &str, pool: &SqlitePool) -> Result<Claims, TokenProblem> {
    let mut claims = match decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    ) {
        Ok(data) => data.claims,
        Err(e) if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) => {
            return Err(TokenProblem::Expired)
        }
        Err(_) => return Err(TokenProblem::Invalid),
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs() as i64;
    let row = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT u.username, u.role, s.expires_at FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id = ? AND s.user_id = ?",
    ).bind(&claims.jti).bind(&claims.sub).fetch_optional(pool).await
        .map_err(|e| {
            tracing::error!("Session lookup failed: {e}");
            TokenProblem::Internal
        })?
        .ok_or(TokenProblem::Revoked)?;
    if row.2 <= now {
        return Err(TokenProblem::Expired);
    }
    claims.username = row.0;
    claims.role = row.1;
    Ok(claims)
}

/// Like [`check_active_jwt`], for REST handlers: any problem is a plain 401.
pub async fn verify_active_jwt(token: &str, secret: &str, pool: &SqlitePool) -> Result<Claims, ServerError> {
    check_active_jwt(token, secret, pool).await.map_err(|problem| match problem {
        TokenProblem::Internal => ServerError::Internal("Session lookup failed".to_string()),
        _ => ServerError::Unauthorized,
    })
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

fn close_session_connections(state: &ArcSharedState, token_ids: &[String]) {
    let connections: Vec<_> = state.sessions.sessions.iter()
        .filter(|session| token_ids.contains(&session.token_id))
        .map(|session| session.connection.clone()).collect();
    for connection in connections { connection.close(0u32.into(), b"token revoked"); }
}

#[derive(Debug, Serialize)]
pub struct SessionDto {
    pub id: String,
    pub device_name: Option<String>,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub current: bool,
}

/// GET /api/auth/sessions: the caller's signed-in devices, most recently seen first.
pub async fn list_sessions_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<Json<Vec<SessionDto>>, ServerError> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let rows = sqlx::query_as::<_, (String, Option<String>, i64, i64)>(
        "SELECT id, device_name, created_at, COALESCE(last_seen_at, created_at) AS seen FROM sessions
         WHERE user_id = ? AND expires_at > ? ORDER BY seen DESC, created_at DESC",
    ).bind(auth.user_id.to_string()).bind(now).fetch_all(&state.pool).await?;
    Ok(Json(rows.into_iter().map(|(id, device_name, created_at, last_seen_at)| SessionDto {
        current: id == auth.session_id, id, device_name, created_at, last_seen_at,
    }).collect()))
}

/// DELETE /api/auth/sessions/{id}: ends one of the caller's own sessions.
pub async fn end_session_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, ServerError> {
    let deleted = sqlx::query("DELETE FROM sessions WHERE id = ? AND user_id = ?")
        .bind(&id).bind(auth.user_id.to_string()).execute(&state.pool).await?;
    if deleted.rows_affected() == 0 {
        return Err(ServerError::NotFound("Session not found".to_string()));
    }
    close_session_connections(&state, &[id]);
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/auth/sessions/revoke-others: ends every session of the caller except this one.
pub async fn revoke_other_sessions_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<StatusCode, ServerError> {
    let ids: Vec<String> = sqlx::query_scalar("DELETE FROM sessions WHERE user_id = ? AND id != ? RETURNING id")
        .bind(auth.user_id.to_string()).bind(&auth.session_id).fetch_all(&state.pool).await?;
    close_session_connections(&state, &ids);
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
    /// Shown in the list of signed-in devices.
    pub device_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Shown in the list of signed-in devices.
    pub device_name: Option<String>,
}

/// Trimmed device label, at most 64 characters; empty means none.
fn clean_device_name(name: Option<&str>) -> Option<String> {
    let name = name?.trim();
    if name.is_empty() { None } else { Some(name.chars().take(64).collect()) }
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
    peer: Option<axum::Extension<crate::limits::PeerAddr>>,
    Json(payload): Json<RegisterRequest>,
) -> Result<impl IntoResponse, ServerError> {
    let username = payload.username.trim().to_lowercase();
    if let Some(axum::Extension(crate::limits::PeerAddr(ip))) = peer {
        state
            .register_hits
            .hit(&ip.to_string(), std::time::Instant::now(), state.config.auth.register_per_hour, std::time::Duration::from_secs(3600))
            .map_err(ServerError::TooManyRequests)?;
    }
    if username.len() < 3 || username.len() > 32 {
        return Err(ServerError::BadRequest(
            "Username must be between 3 and 32 characters".to_string(),
        ));
    }
    if payload.password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ServerError::BadRequest(
            "The password must be at least 8 characters".to_string(),
        ));
    }

    // A blank name (missing, empty or only spaces) is shown as the username.
    let display_name = payload
        .display_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| username.clone());

    let password_hash = hash_password(&state.auth_hash_gate, payload.password.clone()).await?;
    let user_id = Uuid::new_v4();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // One transaction: spending the invite and creating the account stand or fall together.
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    if crate::settings::registration_mode(&state.pool, state.config.auth.registration).await == crate::config::RegistrationMode::Invite {
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
            sqlx::query("INSERT INTO sessions (id, user_id, expires_at, created_at, device_name, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(jti)
                .bind(user_id.to_string())
                .bind(exp)
                .bind(now)
                .bind(clean_device_name(payload.device_name.as_deref()))
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
    peer: Option<axum::Extension<crate::limits::PeerAddr>>,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, ServerError> {
    let peer = peer.map(|axum::Extension(crate::limits::PeerAddr(ip))| ip);
    login_at(state, peer, payload, std::time::Instant::now()).await
}

/// The sign-in with its clock passed in, so tests can move time forward.
async fn login_at(
    state: ArcSharedState,
    peer: Option<std::net::IpAddr>,
    payload: LoginRequest,
    now: std::time::Instant,
) -> Result<impl IntoResponse, ServerError> {
    let state = &state;
    // Every attempt from one address counts, right or wrong, so a flood cannot try passwords for free.
    // A shared address (NAT) gets a budget, not a ban: the window slides and the budget comes back.
    if let Some(ip) = peer {
        state.login_ip_hits.hit(&ip.to_string(), now, LOGIN_IP_LIMIT, LOGIN_IP_WINDOW).map_err(ServerError::TooManyRequests)?;
    }
    // Bound input before allocating a persistent brute-force guard key. Registration
    // uses the same username limit; invalid names never represent existing accounts.
    if payload.username.len() > 128 {
        return Err(ServerError::AuthFailed("Invalid username or password".to_string()));
    }
    let username = payload.username.trim().to_lowercase();
    if username.len() < 3 || username.len() > 32 {
        return Err(ServerError::AuthFailed("Invalid username or password".to_string()));
    }
    // Wrong passwords are counted per login and address; too many lock that pair for a while.
    let guard_key = match peer {
        Some(ip) => format!("{username}|{ip}"),
        None => username.clone(),
    };
    state.login_guard.check(&guard_key, now).map_err(ServerError::TooManyRequests)?;

    let row = sqlx::query_as::<_, (String, String, String, String, Option<i64>, Option<i64>)>(
        "SELECT id, password_hash, COALESCE(NULLIF(TRIM(display_name), ''), username), role, avatar_version, banned_at FROM users WHERE username = ?"
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await?;

    let wrong = |state: &ArcSharedState| -> ServerError {
        let lockout = std::time::Duration::from_secs(state.config.auth.login_lockout_secs);
        match state.login_guard.failure(&guard_key, now, state.config.auth.login_max_failures, lockout) {
            Ok(()) => ServerError::AuthFailed("Invalid username or password".to_string()),
            Err(wait) => ServerError::TooManyRequests(wait),
        }
    };
    let (user_id_str, hash, display_name, role, avatar_version, banned_at) = match row {
        Some(r) => r,
        None => {
            // Same work as a real check, so an unknown login takes as long as a wrong password.
            verify_unknown_login(&state.auth_hash_gate, payload.password.clone()).await?;
            return Err(wrong(state));
        }
    };

    if !verify_password(&state.auth_hash_gate, payload.password.clone(), hash).await? {
        return Err(wrong(state));
    }
    state.login_guard.success(&guard_key);

    if banned_at.is_some() {
        let until: Option<i64> = sqlx::query_scalar("SELECT banned_until FROM users WHERE username = ?").bind(&username).fetch_one(&state.pool).await?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        if until.is_none_or(|end| end > now) {
            return Err(ServerError::Banned);
        }
        // A timed ban that ran out lifts itself.
        sqlx::query("UPDATE users SET banned_at = NULL, banned_until = NULL WHERE username = ?").bind(&username).execute(&state.pool).await?;
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

    sqlx::query("INSERT INTO sessions (id, user_id, expires_at, created_at, device_name, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(jti)
        .bind(&user_id_str)
        .bind(exp)
        .bind(now)
        .bind(clean_device_name(payload.device_name.as_deref()))
        .bind(now)
        .execute(&state.pool)
        .await?;

    let response = AuthResponse {
        quic_port: state.config.server.quic_port,
        token,
        user: UserDto {
            avatar_url: crate::api::avatars::avatar_url(&user_id_str, avatar_version),
            id: user_id_str,
            username,
            display_name,
            role,
        },
    };

    Ok(Json(response))
}

/// Handler for POST /api/auth/refresh: slides the session forward and returns a fresh token.
pub async fn refresh_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
) -> Result<Json<AuthResponse>, ServerError> {
    let user_id_str = auth.user_id.to_string();
    let (username, display_name, role, avatar_version) = sqlx::query_as::<_, (String, String, String, Option<i64>)>(
        "SELECT username, COALESCE(NULLIF(TRIM(display_name), ''), username), role, avatar_version FROM users WHERE id = ?"
    )
    .bind(&user_id_str)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ServerError::Unauthorized)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let exp = now + (state.config.auth.jwt_expiration_hours * 3600);
    let updated = sqlx::query("UPDATE sessions SET expires_at = ?, last_seen_at = ? WHERE id = ? AND user_id = ?")
        .bind(exp)
        .bind(now)
        .bind(&auth.session_id)
        .bind(&user_id_str)
        .execute(&state.pool)
        .await?;
    if updated.rows_affected() != 1 {
        return Err(ServerError::Unauthorized);
    }

    let token = issue_jwt_for_session(
        &auth.user_id,
        &username,
        &role,
        &state.config.auth.jwt_secret,
        state.config.auth.jwt_expiration_hours,
        &auth.session_id,
    )?;

    Ok(Json(AuthResponse {
        quic_port: state.config.server.quic_port,
        token,
        user: UserDto {
            avatar_url: crate::api::avatars::avatar_url(&user_id_str, avatar_version),
            id: user_id_str,
            username,
            display_name,
            role,
        },
    }))
}

/// Handler for GET /api/auth/me
pub async fn me_handler(
    auth: AuthUser,
    State(state): State<ArcSharedState>,
) -> Result<impl IntoResponse, ServerError> {
    let row = sqlx::query_as::<_, (String, String, String, String, Option<i64>)>(
        "SELECT id, username, COALESCE(NULLIF(TRIM(display_name), ''), username), role, avatar_version FROM users WHERE id = ?"
    )
    .bind(auth.user_id.to_string())
    .fetch_optional(&state.pool)
    .await?;

    let (id, username, display_name, role, avatar_version) = match row {
        Some(r) => r,
        None => return Err(ServerError::NotFound("User not found".to_string())),
    };

    let user = UserDto {
        avatar_url: crate::api::avatars::avatar_url(&id, avatar_version),
        id,
        username,
        display_name,
        role,
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
    if name.chars().count() > 32 {
        return Err(ServerError::BadRequest("Display name must be at most 32 characters".into()));
    }
    // A blank name is stored as the username, the same as at registration.
    sqlx::query("UPDATE users SET display_name = COALESCE(NULLIF(?, ''), username) WHERE id = ?")
        .bind(&name)
        .bind(auth.user_id.to_string())
        .execute(&state.pool)
        .await?;
    let name: String = sqlx::query_scalar("SELECT display_name FROM users WHERE id = ?")
        .bind(auth.user_id.to_string())
        .fetch_one(&state.pool)
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
    if request.new_password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ServerError::BadRequest("The password must be at least 8 characters".into()));
    }
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(auth.user_id.to_string())
        .fetch_one(&state.pool)
        .await?;
    if !verify_password(&state.auth_hash_gate, request.current_password.clone(), hash).await? {
        return Err(ServerError::Forbidden);
    }
    let new_hash = hash_password(&state.auth_hash_gate, request.new_password.clone()).await?;
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

#[cfg(test)]
mod login_tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, Instant};

    const PEER: Option<IpAddr> = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)));
    const OTHER_PEER: Option<IpAddr> = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8)));

    /// The hash-check counter is process-wide, so the tests that read it run one at a time.
    fn serial() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    async fn state_with_alice() -> ArcSharedState {
        let pool = crate::db::init_db("sqlite::memory:").await.expect("database");
        let gate = Arc::new(Semaphore::new(2));
        let hash = hash_password(&gate, "password_123".to_string()).await.expect("hash");
        sqlx::query(
            "INSERT INTO users (id, username, display_name, password_hash, role, status, created_at) VALUES (?, 'alice', 'alice', ?, 'member', 'offline', 0)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(hash)
        .execute(&pool)
        .await
        .expect("alice");
        ArcSharedState {
            pool,
            config: ServerConfig::default(),
            sessions: SessionManager::new(),
            login_guard: Default::default(),
            register_hits: Default::default(),
            avatar_hits: Default::default(),
            login_ip_hits: Default::default(),
            auth_hash_gate: Arc::new(Semaphore::new(2)),
        }
    }

    fn attempt(username: &str, password: &str) -> LoginRequest {
        LoginRequest { username: username.into(), password: password.into(), device_name: None }
    }

    fn refusal<T>(result: Result<T, ServerError>) -> ServerError {
        match result {
            Ok(_) => panic!("the sign-in was expected to be refused"),
            Err(error) => error,
        }
    }

    #[tokio::test]
    async fn unknown_login_runs_a_hash_check_and_answers_like_a_wrong_password() {
        let _serial = serial().lock().await;
        let state = state_with_alice().await;
        let before = VERIFY_CALLS.load(std::sync::atomic::Ordering::SeqCst);
        let unknown = refusal(login_at(state.clone(), PEER, attempt("nobody", "wrong-password"), Instant::now()).await);
        let after_unknown = VERIFY_CALLS.load(std::sync::atomic::Ordering::SeqCst);
        let wrong = refusal(login_at(state.clone(), PEER, attempt("alice", "wrong-password"), Instant::now()).await);
        assert_eq!(after_unknown - before, 1, "an unknown login must check one hash");
        assert!(matches!(unknown, ServerError::AuthFailed(ref m) if m == "Invalid username or password"));
        assert!(matches!(wrong, ServerError::AuthFailed(ref m) if m == "Invalid username or password"));
    }

    #[tokio::test]
    async fn twenty_attempts_per_address_then_refused_until_ten_minutes_pass() {
        let state = state_with_alice().await;
        let t0 = Instant::now();
        // Short names are refused before any password work, but every attempt still counts.
        for _ in 0..LOGIN_IP_LIMIT {
            assert!(matches!(refusal(login_at(state.clone(), PEER, attempt("ab", "x"), t0).await), ServerError::AuthFailed(_)));
        }
        let refused = refusal(login_at(state.clone(), PEER, attempt("ab", "x"), t0).await);
        assert!(
            matches!(refused, ServerError::TooManyRequests(wait) if (1..=601).contains(&wait)),
            "the 21st attempt in ten minutes is refused with a wait"
        );
        let other = refusal(login_at(state.clone(), OTHER_PEER, attempt("ab", "x"), t0).await);
        assert!(matches!(other, ServerError::AuthFailed(_)), "another address has its own budget");
        let later = refusal(login_at(state.clone(), PEER, attempt("ab", "x"), t0 + Duration::from_secs(600)).await);
        assert!(matches!(later, ServerError::AuthFailed(_)), "after ten minutes the address may try again");
    }

    #[tokio::test]
    async fn a_panicking_hash_job_is_an_internal_error_not_a_panic() {
        let gate = Arc::new(Semaphore::new(1));
        let result = run_gated(&gate, || -> u8 { panic!("hash job failed") }).await;
        assert!(matches!(result, Err(ServerError::Internal(_))));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_permit_runs_hash_jobs_one_after_another() {
        let gate = Arc::new(Semaphore::new(1));
        let spans = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let gate = gate.clone();
            let spans = spans.clone();
            tasks.push(tokio::spawn(async move {
                run_gated(&gate, move || {
                    let start = Instant::now();
                    std::thread::sleep(Duration::from_millis(150));
                    spans.lock().expect("spans").push((start, Instant::now()));
                })
                .await
            }));
        }
        for task in tasks {
            task.await.expect("task").expect("job");
        }
        let mut spans = spans.lock().expect("spans").clone();
        spans.sort_by_key(|span| span.0);
        assert_eq!(spans.len(), 2);
        assert!(spans[1].0 >= spans[0].1, "the second job started before the first finished");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dropped_request_keeps_its_permit_until_its_hash_finishes() {
        let gate = Arc::new(Semaphore::new(1));
        let a_end = Arc::new(std::sync::Mutex::new(None::<Instant>));
        let b_start = Arc::new(std::sync::Mutex::new(None::<Instant>));
        let a = tokio::spawn({
            let gate = gate.clone();
            let a_end = a_end.clone();
            async move {
                run_gated(&gate, move || {
                    std::thread::sleep(Duration::from_millis(300));
                    *a_end.lock().expect("a_end") = Some(Instant::now());
                })
                .await
            }
        });
        // Let job A reach the blocking pool, then drop the request that started it.
        tokio::time::sleep(Duration::from_millis(50)).await;
        a.abort();
        let b = b_start.clone();
        run_gated(&gate, move || {
            *b.lock().expect("b_start") = Some(Instant::now());
        })
        .await
        .expect("job B");
        let a_done = a_end.lock().expect("a_end").expect("job A finished before B was allowed");
        let b_began = b_start.lock().expect("b_start").expect("job B ran");
        assert!(b_began >= a_done, "job B started while job A's hash was still running");
    }
}
