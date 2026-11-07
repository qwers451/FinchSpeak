//! REST API Integration Tests.
//!
//! Tests:
//! - User registration (admin and member)
//! - Login with Argon2id and JWT verification
//! - Protected /api/auth/me endpoint
//! - Channel listing and creation (admin only)
//! - Chat message history retrieval
//! - File upload and download

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::util::ServiceExt;

use finch_server::api::create_router;
use finch_server::db::init_db;

/// These scenarios create several users without invitations.
fn open_registration() -> finch_server::config::ServerConfig {
    let mut config = finch_server::config::ServerConfig::default();
    config.auth.registration = finch_server::config::RegistrationMode::Open;
    config
}


#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aud038_concurrent_first_registrations_create_one_admin() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for index in 0..8 {
        let app = app.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let request = Request::builder().method("POST").uri("/api/auth/register")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "username": format!("user{index}"), "password": "password_123" }).to_string())).unwrap();
            app.oneshot(request).await.unwrap().status()
        }));
    }
    for task in tasks { assert_eq!(task.await?, StatusCode::CREATED); }
    let admins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'").fetch_one(&pool).await?;
    assert_eq!(admins, 1, "exactly one concurrent registration must become admin");
    Ok(())
}

#[tokio::test]
async fn aud039_deleted_session_rejects_its_still_signed_token() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let request = Request::builder().method("POST").uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"username":"alice", "password":"password_123"}).to_string()))?;
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await?.to_bytes();
    let auth: Value = serde_json::from_slice(&body)?;
    let token = auth["token"].as_str().unwrap();
    sqlx::query("DELETE FROM sessions").execute(&pool).await?;
    let request = Request::builder().uri("/api/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?;
    assert_eq!(app.oneshot(request).await?.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn aud039_logout_revokes_only_the_requested_token() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let mut tokens = Vec::new();
    for path in ["/api/auth/register", "/api/auth/login"] {
        let request = Request::builder().method("POST").uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"username":"alice", "password":"password_123"}).to_string()))?;
        let response = app.clone().oneshot(request).await?;
        assert!(response.status().is_success());
        let body = response.into_body().collect().await?.to_bytes();
        tokens.push(serde_json::from_slice::<Value>(&body)?["token"].as_str().unwrap().to_string());
    }
    let request = Request::builder().method("POST").uri("/api/auth/logout")
        .header(header::AUTHORIZATION, format!("Bearer {}", tokens[0]))
        .body(Body::empty())?;
    assert_eq!(app.clone().oneshot(request).await?.status(), StatusCode::NO_CONTENT);
    for (token, status) in tokens.iter().zip([StatusCode::UNAUTHORIZED, StatusCode::OK]) {
        let request = Request::builder().uri("/api/auth/me")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())?;
        assert_eq!(app.clone().oneshot(request).await?.status(), status);
    }
    Ok(())
}

#[tokio::test]
async fn test_full_api_lifecycle() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let config = open_registration();
    let app = create_router(pool.clone(), config, finch_server::session::SessionManager::new());

    // ─── 1. Register First User (Admin) ─────────────────────────────────────
    let register_admin = json!({
        "username": "admin",
        "password": "admin_password_123",
        "display_name": "Server Admin"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&register_admin)?))?;

    let res = app.clone().oneshot(req).await?;
    assert_eq!(res.status(), StatusCode::CREATED);

    let body_bytes = res.into_body().collect().await?.to_bytes();
    let admin_auth: Value = serde_json::from_slice(&body_bytes)?;
    let admin_token = admin_auth["token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Admin token missing"))?
        .to_string();
    assert_eq!(admin_auth["user"]["role"], "admin");
    assert_eq!(admin_auth["user"]["username"], "admin");

    // ─── 2. Register Duplicate User (Conflict 409) ──────────────────────────
    let req_dup = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&register_admin)?))?;

    let res_dup = app.clone().oneshot(req_dup).await?;
    assert_eq!(res_dup.status(), StatusCode::CONFLICT);

    // ─── 3. Register Second User (Member) ───────────────────────────────────
    let register_member = json!({
        "username": "alice",
        "password": "alice_password_123",
        "display_name": "Alice"
    });

    let req_member = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&register_member)?))?;

    let res_member = app.clone().oneshot(req_member).await?;
    assert_eq!(res_member.status(), StatusCode::CREATED);

    let member_body = res_member.into_body().collect().await?.to_bytes();
    let member_auth: Value = serde_json::from_slice(&member_body)?;
    let member_token = member_auth["token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Member token missing"))?
        .to_string();
    assert_eq!(member_auth["user"]["role"], "member");

    // ─── 4. Login with Correct and Bad Passwords ────────────────────────────
    let login_bad = json!({
        "username": "alice",
        "password": "wrong_password"
    });
    let req_bad = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&login_bad)?))?;
    let res_bad = app.clone().oneshot(req_bad).await?;
    assert_eq!(res_bad.status(), StatusCode::UNAUTHORIZED);

    let login_good = json!({
        "username": "alice",
        "password": "alice_password_123"
    });
    let req_good = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&login_good)?))?;
    let res_good = app.clone().oneshot(req_good).await?;
    assert_eq!(res_good.status(), StatusCode::OK);

    // ─── 5. Test /api/auth/me ───────────────────────────────────────────────
    let req_me = Request::builder()
        .method("GET")
        .uri("/api/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {member_token}"))
        .body(Body::empty())?;
    let res_me = app.clone().oneshot(req_me).await?;
    assert_eq!(res_me.status(), StatusCode::OK);
    let me_body = res_me.into_body().collect().await?.to_bytes();
    let me_json: Value = serde_json::from_slice(&me_body)?;
    assert_eq!(me_json["username"], "alice");

    // ─── 6. List Seeded Channels ────────────────────────────────────────────
    let req_channels = Request::builder()
        .method("GET")
        .uri("/api/channels")
        .header(header::AUTHORIZATION, format!("Bearer {member_token}"))
        .body(Body::empty())?;
    let res_channels = app.clone().oneshot(req_channels).await?;
    assert_eq!(res_channels.status(), StatusCode::OK);

    let ch_body = res_channels.into_body().collect().await?.to_bytes();
    let channels: Vec<Value> = serde_json::from_slice(&ch_body)?;
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0]["name"], "general");
    assert_eq!(channels[0]["short_id"], 1);
    assert_eq!(channels[1]["name"], "General Voice");
    assert_eq!(channels[1]["short_id"], 2);

    let general_id = channels[0]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("General channel ID missing"))?
        .to_string();

    // ─── 7. Create Channel: Member Forbidden (403), Admin Allowed (201) ─────
    let create_ch = json!({
        "name": "dev-chat",
        "channel_type": "text",
        "topic": "Developer discussions"
    });

    let req_forbidden = Request::builder()
        .method("POST")
        .uri("/api/channels")
        .header(header::AUTHORIZATION, format!("Bearer {member_token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&create_ch)?))?;
    let res_forbidden = app.clone().oneshot(req_forbidden).await?;
    assert_eq!(res_forbidden.status(), StatusCode::FORBIDDEN);

    let req_create = Request::builder()
        .method("POST")
        .uri("/api/channels")
        .header(header::AUTHORIZATION, format!("Bearer {admin_token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&create_ch)?))?;
    let res_create = app.clone().oneshot(req_create).await?;
    assert_eq!(res_create.status(), StatusCode::CREATED);
    let create_body = res_create.into_body().collect().await?.to_bytes();
    let created_ch: Value = serde_json::from_slice(&create_body)?;
    assert_eq!(created_ch["short_id"], 3);
    assert_eq!(created_ch["name"], "dev-chat");

    // ─── 8. Query Message History ───────────────────────────────────────────
    let req_hist = Request::builder()
        .method("GET")
        .uri(format!("/api/channels/{general_id}/messages"))
        .header(header::AUTHORIZATION, format!("Bearer {member_token}"))
        .body(Body::empty())?;
    let res_hist = app.clone().oneshot(req_hist).await?;
    assert_eq!(res_hist.status(), StatusCode::OK);
    let hist_body = res_hist.into_body().collect().await?.to_bytes();
    let hist_json: Value = serde_json::from_slice(&hist_body)?;
    assert_eq!(hist_json["has_more"], false);

    Ok(())
}

#[tokio::test]
async fn health_reports_ok_without_credentials() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let response = app.oneshot(Request::builder().uri("/health").body(Body::empty())?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["uptime_secs"].is_u64());
    Ok(())
}

#[tokio::test]
async fn health_does_not_leak_users_or_channels() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let response = app.oneshot(Request::builder().uri("/health").body(Body::empty())?).await?;
    let text = String::from_utf8(response.into_body().collect().await?.to_bytes().to_vec())?;
    assert!(!text.contains("general") && !text.contains("user"), "health must stay minimal: {text}");
    Ok(())
}

async fn register_alice(app: &axum::Router) -> anyhow::Result<String> {
    let request = Request::builder().method("POST").uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"username":"alice", "password":"password_123"}).to_string()))?;
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await?.to_bytes();
    Ok(serde_json::from_slice::<Value>(&body)?["token"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn refresh_extends_the_same_session() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let old_token = register_alice(&app).await?;
    sqlx::query("UPDATE sessions SET expires_at = expires_at - 3600").execute(&pool).await?;
    let before: i64 = sqlx::query_scalar("SELECT expires_at FROM sessions").fetch_one(&pool).await?;
    // JWT exp has one-second resolution; wait so the refreshed token differs.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let request = Request::builder().method("POST").uri("/api/auth/refresh")
        .header(header::AUTHORIZATION, format!("Bearer {old_token}"))
        .body(Body::empty())?;
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await?.to_bytes();
    let new_token = serde_json::from_slice::<Value>(&body)?["token"].as_str().unwrap().to_string();
    assert_ne!(new_token, old_token);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions").fetch_one(&pool).await?;
    assert_eq!(count, 1);
    let after: i64 = sqlx::query_scalar("SELECT expires_at FROM sessions").fetch_one(&pool).await?;
    assert!(after > before);
    let request = Request::builder().uri("/api/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {new_token}"))
        .body(Body::empty())?;
    assert_eq!(app.oneshot(request).await?.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn refresh_rejects_a_revoked_session() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let token = register_alice(&app).await?;
    sqlx::query("DELETE FROM sessions").execute(&pool).await?;
    let request = Request::builder().method("POST").uri("/api/auth/refresh")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?;
    assert_eq!(app.oneshot(request).await?.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn refreshed_token_survives_logout_of_other_device() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let token0 = register_alice(&app).await?;
    let request = Request::builder().method("POST").uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"username":"alice", "password":"password_123"}).to_string()))?;
    let response = app.clone().oneshot(request).await?;
    assert!(response.status().is_success());
    let body = response.into_body().collect().await?.to_bytes();
    let token1 = serde_json::from_slice::<Value>(&body)?["token"].as_str().unwrap().to_string();
    let request = Request::builder().method("POST").uri("/api/auth/logout")
        .header(header::AUTHORIZATION, format!("Bearer {token0}"))
        .body(Body::empty())?;
    assert_eq!(app.clone().oneshot(request).await?.status(), StatusCode::NO_CONTENT);
    let request = Request::builder().method("POST").uri("/api/auth/refresh")
        .header(header::AUTHORIZATION, format!("Bearer {token1}"))
        .body(Body::empty())?;
    assert_eq!(app.oneshot(request).await?.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn expired_and_revoked_tokens_are_told_apart() -> anyhow::Result<()> {
    use finch_server::api::auth::{check_active_jwt, issue_jwt_for_session, TokenProblem};
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let token = register_alice(&app).await?;
    let secret = open_registration().auth.jwt_secret;
    let claims = check_active_jwt(&token, &secret, &pool).await.map_err(|_| anyhow::anyhow!("fresh token rejected"))?;
    let user_id = uuid::Uuid::parse_str(&claims.sub)?;

    let stale = issue_jwt_for_session(&user_id, "alice", "admin", &secret, -1, &claims.jti)?;
    assert!(matches!(check_active_jwt(&stale, &secret, &pool).await, Err(TokenProblem::Expired)));

    sqlx::query("UPDATE sessions SET expires_at = 0").execute(&pool).await?;
    assert!(matches!(check_active_jwt(&token, &secret, &pool).await, Err(TokenProblem::Expired)));

    sqlx::query("DELETE FROM sessions").execute(&pool).await?;
    assert!(matches!(check_active_jwt(&token, &secret, &pool).await, Err(TokenProblem::Revoked)));
    assert!(matches!(check_active_jwt("garbage", &secret, &pool).await, Err(TokenProblem::Invalid)));
    Ok(())
}

#[tokio::test]
async fn database_failure_is_not_reported_as_a_bad_token() -> anyhow::Result<()> {
    use finch_server::api::auth::{check_active_jwt, TokenProblem};
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let token = register_alice(&app).await?;
    let secret = open_registration().auth.jwt_secret;
    pool.close().await;
    assert!(matches!(check_active_jwt(&token, &secret, &pool).await, Err(TokenProblem::Internal)));
    Ok(())
}

async fn auth_json(app: &axum::Router, path: &str, body: Value) -> anyhow::Result<Value> {
    let request = Request::builder().method("POST").uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))?;
    let response = app.clone().oneshot(request).await?;
    assert!(response.status().is_success(), "{path} -> {}", response.status());
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(serde_json::from_slice(&bytes)?)
}

async fn call(app: &axum::Router, method: &str, uri: &str, token: &str) -> anyhow::Result<(StatusCode, Value)> {
    let request = Request::builder().method(method).uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?;
    let response = app.clone().oneshot(request).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
}

#[tokio::test]
async fn sessions_list_marks_the_current_one() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let first = register_alice(&app).await?;
    let laptop = auth_json(&app, "/api/auth/login", json!({"username":"alice","password":"password_123","device_name":"LAPTOP"})).await?;
    let laptop_token = laptop["token"].as_str().unwrap().to_string();
    let (status, list) = call(&app, "GET", "/api/auth/sessions", &laptop_token).await?;
    assert_eq!(status, StatusCode::OK);
    let items = list.as_array().unwrap();
    assert_eq!(items.len(), 2);
    let current: Vec<_> = items.iter().filter(|s| s["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["device_name"], "LAPTOP");
    let (_, list) = call(&app, "GET", "/api/auth/sessions", &first).await?;
    let me = list.as_array().unwrap().iter().find(|s| s["current"] == true).unwrap();
    assert!(me["device_name"].is_null());
    Ok(())
}

#[tokio::test]
async fn old_sessions_show_created_at_as_last_seen() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let token = register_alice(&app).await?;
    sqlx::query("UPDATE sessions SET last_seen_at = NULL").execute(&pool).await?;
    let (_, list) = call(&app, "GET", "/api/auth/sessions", &token).await?;
    let item = &list.as_array().unwrap()[0];
    assert_eq!(item["last_seen_at"], item["created_at"]);
    assert!(item["created_at"].as_i64().unwrap() > 0);
    Ok(())
}

#[tokio::test]
async fn deleting_another_users_session_is_404() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let alice = register_alice(&app).await?;
    let bob = auth_json(&app, "/api/auth/register", json!({"username":"bob","password":"password_123"})).await?;
    let bob_token = bob["token"].as_str().unwrap().to_string();
    let (_, list) = call(&app, "GET", "/api/auth/sessions", &bob_token).await?;
    let bob_session = list[0]["id"].as_str().unwrap().to_string();
    let (status, _) = call(&app, "DELETE", &format!("/api/auth/sessions/{bob_session}"), &alice).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "GET", "/api/auth/me", &bob_token).await?;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, "DELETE", &format!("/api/auth/sessions/{bob_session}"), &bob_token).await?;
    assert_eq!(status, StatusCode::NO_CONTENT);
    Ok(())
}

#[tokio::test]
async fn revoke_others_keeps_only_current() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let first = register_alice(&app).await?;
    let second = auth_json(&app, "/api/auth/login", json!({"username":"alice","password":"password_123"})).await?;
    let second = second["token"].as_str().unwrap().to_string();
    let (status, _) = call(&app, "POST", "/api/auth/sessions/revoke-others", &second).await?;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "GET", "/api/auth/me", &first).await?.0, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "GET", "/api/auth/me", &second).await?.0, StatusCode::OK);
    let (_, list) = call(&app, "GET", "/api/auth/sessions", &second).await?;
    assert_eq!(list.as_array().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn refresh_touches_last_seen() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let token = register_alice(&app).await?;
    sqlx::query("UPDATE sessions SET last_seen_at = 1").execute(&pool).await?;
    assert_eq!(call(&app, "POST", "/api/auth/refresh", &token).await?.0, StatusCode::OK);
    let seen: i64 = sqlx::query_scalar("SELECT last_seen_at FROM sessions").fetch_one(&pool).await?;
    assert!(seen > 1_000_000);
    Ok(())
}

/// Register a user and return (token, user id).
async fn register_named(app: &axum::Router, name: &str, password: &str) -> anyhow::Result<(String, String)> {
    let body = auth_json(app, "/api/auth/register", json!({"username": name, "password": password})).await?;
    Ok((body["token"].as_str().unwrap().to_string(), body["user"]["id"].as_str().unwrap().to_string()))
}

async fn post_empty(app: &axum::Router, uri: &str, token: &str) -> anyhow::Result<(StatusCode, Value)> {
    let request = Request::builder().method("POST").uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())?;
    let response = app.clone().oneshot(request).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
}

async fn login_status(app: &axum::Router, name: &str, password: &str) -> anyhow::Result<StatusCode> {
    let request = Request::builder().method("POST").uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"username": name, "password": password}).to_string()))?;
    Ok(app.clone().oneshot(request).await?.status())
}

#[tokio::test]
async fn admin_reset_issues_a_working_temporary_password() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let (admin, _) = register_named(&app, "admin", "admin_pass_1").await?;
    let (alice, alice_id) = register_named(&app, "alice", "old_password_1").await?;

    let (status, body) = post_empty(&app, &format!("/api/admin/users/{alice_id}/password-reset"), &admin).await?;
    assert_eq!(status, StatusCode::OK);
    let temporary = body["temporary_password"].as_str().unwrap().to_string();
    assert_eq!(temporary.chars().count(), 12);
    assert!(temporary.chars().all(|c| "ABCDEFGHJKMNPQRSTUVWXYZ23456789".contains(c)), "{temporary}");

    assert_eq!(login_status(&app, "alice", &temporary).await?, StatusCode::OK);
    assert_eq!(login_status(&app, "alice", "old_password_1").await?, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&app, "GET", "/api/auth/me", &alice).await?.0, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn member_cannot_reset() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let (_admin, _) = register_named(&app, "admin", "admin_pass_1").await?;
    let (alice, _) = register_named(&app, "alice", "alice_pass_1").await?;
    let (_bob, bob_id) = register_named(&app, "bob", "bob_pass_123").await?;
    let (status, _) = post_empty(&app, &format!("/api/admin/users/{bob_id}/password-reset"), &alice).await?;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(login_status(&app, "bob", "bob_pass_123").await?, StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn admin_cannot_reset_another_admin() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let (admin, _) = register_named(&app, "admin", "admin_pass_1").await?;
    let (_second, second_id) = register_named(&app, "second", "second_pass_1").await?;
    sqlx::query("UPDATE users SET role = 'admin' WHERE username = 'second'").execute(&pool).await?;
    let (status, _) = post_empty(&app, &format!("/api/admin/users/{second_id}/password-reset"), &admin).await?;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(login_status(&app, "second", "second_pass_1").await?, StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn reset_of_unknown_user_is_404() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let (admin, _) = register_named(&app, "admin", "admin_pass_1").await?;
    let missing = uuid::Uuid::new_v4();
    let (status, _) = post_empty(&app, &format!("/api/admin/users/{missing}/password-reset"), &admin).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn reset_is_audited_without_the_password() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let (admin, _) = register_named(&app, "admin", "admin_pass_1").await?;
    let (_alice, alice_id) = register_named(&app, "alice", "alice_pass_1").await?;
    let (status, body) = post_empty(&app, &format!("/api/admin/users/{alice_id}/password-reset"), &admin).await?;
    assert_eq!(status, StatusCode::OK);
    let temporary = body["temporary_password"].as_str().unwrap().to_string();
    let rows: Vec<(String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT action, target, details FROM audit_log WHERE action = 'password_reset'").fetch_all(&pool).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.as_deref(), Some("alice"));
    assert!(!rows[0].2.clone().unwrap_or_default().contains(&temporary));
    Ok(())
}

#[tokio::test]
async fn short_password_is_rejected_at_registration() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let request = Request::builder().method("POST").uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"username":"alice", "password":"1234567"}).to_string()))?;
    let response = app.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(body["error"], "The password must be at least 8 characters");
    Ok(())
}

#[tokio::test]
async fn password_length_counts_characters_not_bytes() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    // Four Cyrillic letters are eight bytes but only four characters.
    for (name, password, expected) in [("dana", "абвг", StatusCode::BAD_REQUEST), ("erik", "абвгдеж", StatusCode::BAD_REQUEST), ("fay", "абвгдежз", StatusCode::CREATED)] {
        let request = Request::builder().method("POST").uri("/api/auth/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"username": name, "password": password}).to_string()))?;
        assert_eq!(app.clone().oneshot(request).await?.status(), expected, "{password}");
    }
    Ok(())
}

#[tokio::test]
async fn change_password_counts_characters_not_bytes() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool, open_registration(), finch_server::session::SessionManager::new());
    let token = register_alice(&app).await?;
    let change = |new_password: &str| {
        Request::builder().method("POST").uri("/api/auth/password")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"current_password": "password_123", "new_password": new_password}).to_string()))
    };
    let short = app.clone().oneshot(change("абвг")?).await?;
    assert_eq!(short.status(), StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&short.into_body().collect().await?.to_bytes())?;
    assert_eq!(body["error"], "The password must be at least 8 characters");
    assert_eq!(app.clone().oneshot(change("абвгдежз")?).await?.status(), StatusCode::NO_CONTENT);
    Ok(())
}

#[tokio::test]
async fn blank_display_name_falls_back_to_the_username() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let app = create_router(pool.clone(), open_registration(), finch_server::session::SessionManager::new());
    let (admin, _) = register_named(&app, "admin", "admin_pass_1").await?;
    let blank = auth_json(&app, "/api/auth/register", json!({"username": "dana", "password": "dana_pass_1", "display_name": ""})).await?;
    assert_eq!(blank["user"]["display_name"], "dana");
    let spaces = auth_json(&app, "/api/auth/register", json!({"username": "erik", "password": "erik_pass_1", "display_name": "   "})).await?;
    assert_eq!(spaces["user"]["display_name"], "erik");
    let erik_token = spaces["token"].as_str().unwrap().to_string();

    // A row stored with a blank name before this fix still reads back as the username.
    sqlx::query("UPDATE users SET display_name = '' WHERE username = 'dana'").execute(&pool).await?;
    let channel: String = sqlx::query_scalar("SELECT id FROM channels WHERE name = 'general'").fetch_one(&pool).await?;
    sqlx::query("INSERT INTO messages (id, channel_id, author_id, content, created_at) SELECT 'm-blank', ?, id, 'hi', 1 FROM users WHERE username = 'dana'")
        .bind(&channel).execute(&pool).await?;
    let (status, history) = call(&app, "GET", &format!("/api/channels/{channel}/messages"), &admin).await?;
    assert_eq!(status, StatusCode::OK);
    let message = history["messages"].as_array().unwrap().iter().find(|m| m["content"] == "hi").unwrap();
    assert_eq!(message["author_name"], "dana");

    // The profile change treats a blank name the same way.
    let request = Request::builder().method("PATCH").uri("/api/auth/me")
        .header(header::AUTHORIZATION, format!("Bearer {erik_token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"display_name": "  "}).to_string()))?;
    let response = app.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(body["display_name"], "erik");
    Ok(())
}
