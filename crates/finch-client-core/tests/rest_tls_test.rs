//! REST credentials must only cross a confirmed, pinned TLS connection.
use std::sync::Arc;
use dashmap::DashMap;
use finch_client_core::{api::ApiClient, error::ClientError, tofu::{load_fingerprints, save_fingerprints}};
use finch_server::{config::ServerConfig, launch::launch};

#[tokio::test]
async fn rest_rejects_plaintext_before_sending_credentials() {
    assert!(ApiClient::new("http://127.0.0.1:8443").is_err());
}

#[tokio::test]
async fn passwords_require_confirmation_and_persisted_pin_rejects_changed_certificate() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("finch-rest-tls-{}", uuid::Uuid::new_v4()));
    let mut config = ServerConfig::default();
    config.server.host = "127.0.0.1".into();
    config.server.http_port = 0;
    config.server.quic_port = 0;
    config.server.tls_dir = dir.join("tls").to_string_lossy().into();
    config.database.url = "sqlite::memory:".into();
    config.auth.jwt_secret = "rest-tls-test-secret-0123456789".into();
    let server = launch(config.clone()).await?;
    let url = format!("https://{}", server.http_addr);
    let pins = Arc::new(DashMap::new());
    let api = ApiClient::with_tofu(&url, pins.clone())?;
    let fingerprint = match api.register("alice", "password_123", "Alice").await {
        Err(ClientError::UntrustedServer { fingerprint, .. }) => fingerprint,
        other => panic!("expected first-use rejection, got {other:?}"),
    };
    assert!(pins.is_empty(), "discovery must never trust automatically");
    pins.insert("127.0.0.1".into(), fingerprint);
    let path = dir.join("known_servers");
    save_fingerprints(&path, &pins)?;
    let api = ApiClient::with_tofu(&url, Arc::new(load_fingerprints(&path)?))?;
    // If the first request had reached the registration handler this would conflict.
    api.register("alice", "password_123", "Alice").await?;
    server.shutdown().await;
    let server = launch(config.clone()).await?;
    let url = format!("https://{}", server.http_addr);
    ApiClient::with_tofu(&url, Arc::new(load_fingerprints(&path)?))?.verify_trust().await?;
    server.shutdown().await;
    config.server.tls_dir = dir.join("attacker-tls").to_string_lossy().into();
    let attacker = launch(config).await?;
    let api = ApiClient::with_tofu(&format!("https://{}", attacker.http_addr), Arc::new(load_fingerprints(&path)?))?;
    assert!(matches!(api.login("alice", "password_123").await, Err(ClientError::FingerprintMismatch { .. })));
    attacker.shutdown().await;
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
