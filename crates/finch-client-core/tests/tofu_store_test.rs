//! Pinned server fingerprints: confirmation on first contact, mismatch detection, persistence (AUD-005).

use std::sync::Arc;

use dashmap::DashMap;
use finch_client_core::api::ApiClient;
use finch_client_core::error::ClientError;
use finch_client_core::session::{connect, ConnectParams};
use finch_client_core::tofu::{load_fingerprints, save_fingerprints};
use finch_server::config::ServerConfig;
use finch_server::launch::{launch, RunningServer};
use uuid::Uuid;

async fn start_server() -> anyhow::Result<(RunningServer, String)> {
    let dir = std::env::temp_dir().join(format!("finch-tofu-{}", Uuid::new_v4()));
    let mut config = ServerConfig::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.http_port = 0;
    config.server.quic_port = 0;
    config.server.tls_dir = dir.join("tls").to_string_lossy().to_string();
    config.database.url = "sqlite::memory:".to_string();
    config.storage.upload_dir = dir.join("uploads").to_string_lossy().to_string();
    config.auth.jwt_secret = "tofu-store-test-secret-0123456789".to_string();
    let server = launch(config).await?;
    let pins = Arc::new(DashMap::new());
    pins.insert("127.0.0.1".to_string(), server.certificate_fingerprint.clone());
    let api = ApiClient::with_tofu(&format!("https://{}", server.http_addr), pins)?;
    let token = api.register("alice", "password_123", "Alice").await?.token;
    Ok((server, token))
}

fn params(server: &RunningServer, token: &str) -> ConnectParams {
    ConnectParams {
        quic_addr: server.quic_addr,
        hostname: "localhost".to_string(),
        token: token.to_string(),
        require_known_fingerprint: true,
    }
}

#[tokio::test]
async fn unknown_server_requires_confirmation() -> anyhow::Result<()> {
    let (server, token) = start_server().await?;
    let store = Arc::new(DashMap::new());

    let fingerprint = match connect(params(&server, &token), store.clone()).await {
        Err(ClientError::UntrustedServer { hostname, fingerprint }) => {
            assert_eq!(hostname, "localhost");
            fingerprint
        }
        Err(other) => panic!("expected UntrustedServer, got {other:?}"),
        Ok(_) => panic!("unknown server must not be trusted silently"),
    };
    assert!(store.is_empty(), "fingerprint must not be stored before the user confirms");

    // User confirms the fingerprint → the connection succeeds.
    store.insert("localhost".to_string(), fingerprint);
    connect(params(&server, &token), store).await?;
    server.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn changed_fingerprint_is_reported() -> anyhow::Result<()> {
    let (server, token) = start_server().await?;
    let store = Arc::new(DashMap::new());
    store.insert("localhost".to_string(), "AA:BB".to_string());

    match connect(params(&server, &token), store).await {
        Err(ClientError::FingerprintMismatch { expected, actual, .. }) => {
            assert_eq!(expected, "AA:BB");
            assert_ne!(actual, "AA:BB");
        }
        Err(other) => panic!("expected FingerprintMismatch, got {other:?}"),
        Ok(_) => panic!("changed certificate must be rejected"),
    }
    server.shutdown().await;
    Ok(())
}

#[test]
fn fingerprints_roundtrip_through_file() -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(format!("finch-known-{}", Uuid::new_v4())).join("known_servers.txt");
    let store = DashMap::new();
    store.insert("chat.example.org".to_string(), "01:02:03".to_string());
    store.insert("localhost".to_string(), "AA:BB:CC".to_string());

    save_fingerprints(&path, &store)?;
    let loaded = load_fingerprints(&path)?;
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded.get("chat.example.org").map(|v| v.clone()), Some("01:02:03".to_string()));
    assert_eq!(loaded.get("localhost").map(|v| v.clone()), Some("AA:BB:CC".to_string()));

    // A missing file is an empty store, not an error (first launch).
    assert!(load_fingerprints(&path.with_file_name("absent.txt"))?.is_empty());
    Ok(())
}
