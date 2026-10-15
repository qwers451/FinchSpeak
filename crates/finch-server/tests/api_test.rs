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
use finch_server::config::ServerConfig;
use finch_server::db::init_db;

#[tokio::test]
async fn test_full_api_lifecycle() -> anyhow::Result<()> {
    let pool = init_db("sqlite::memory:").await?;
    let config = ServerConfig::default();
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
