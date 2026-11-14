//! CORS policy: only the desktop app's webview origins and the configured `[server] allowed_origins`
//! may read API responses from a browser. Requests without an Origin header (the native Rust client)
//! are not affected by CORS at all.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use tower::util::ServiceExt;

use finch_server::api::create_router;
use finch_server::config::ServerConfig;
use finch_server::db::init_db;
use finch_server::session::SessionManager;

async fn app_with(config: ServerConfig) -> Router {
    let pool = init_db("sqlite::memory:").await.expect("in-memory database");
    create_router(pool, config, SessionManager::new())
}

async fn send(app: &Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.expect("router responds")
}

fn health(origin: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(Method::GET).uri("/health");
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    builder.body(Body::empty()).expect("request")
}

fn preflight(origin: &str) -> Request<Body> {
    Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/auth/me")
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PATCH")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization,content-type")
        .body(Body::empty())
        .expect("request")
}

fn allow_origin(response: &Response) -> Option<String> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .map(|value| value.to_str().expect("ascii header").to_string())
}

#[tokio::test]
async fn foreign_origin_gets_no_allow_origin_on_a_simple_request() -> anyhow::Result<()> {
    let app = app_with(ServerConfig::default()).await;
    let response = send(&app, health(Some("https://evil.example"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(allow_origin(&response), None);
    Ok(())
}

#[tokio::test]
async fn foreign_origin_gets_no_allow_origin_on_preflight() -> anyhow::Result<()> {
    let app = app_with(ServerConfig::default()).await;
    let response = send(&app, preflight("https://evil.example")).await;
    assert_eq!(allow_origin(&response), None);
    Ok(())
}

#[tokio::test]
async fn desktop_webview_origins_are_allowed() -> anyhow::Result<()> {
    let app = app_with(ServerConfig::default()).await;
    for origin in ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"] {
        let response = send(&app, preflight(origin)).await;
        assert_eq!(allow_origin(&response).as_deref(), Some(origin), "origin {origin}");
    }
    Ok(())
}

#[tokio::test]
async fn preflight_allows_the_methods_the_api_uses() -> anyhow::Result<()> {
    let app = app_with(ServerConfig::default()).await;
    let response = send(&app, preflight("tauri://localhost")).await;
    let methods = response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
        .expect("allow-methods header")
        .to_str()?
        .to_ascii_uppercase();
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
        assert!(methods.contains(method), "{method} missing from {methods}");
    }
    Ok(())
}

#[tokio::test]
async fn configured_origin_is_allowed_and_others_are_not() -> anyhow::Result<()> {
    let mut config = ServerConfig::default();
    config.server.allowed_origins = vec!["https://finch.example.com".to_string()];
    let app = app_with(config).await;

    let allowed = send(&app, preflight("https://finch.example.com")).await;
    assert_eq!(allow_origin(&allowed).as_deref(), Some("https://finch.example.com"));

    let other = send(&app, preflight("https://other.example.com")).await;
    assert_eq!(allow_origin(&other), None);
    Ok(())
}

#[tokio::test]
async fn request_without_origin_still_works() -> anyhow::Result<()> {
    let app = app_with(ServerConfig::default()).await;
    let response = send(&app, health(None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(allow_origin(&response), None);
    Ok(())
}
