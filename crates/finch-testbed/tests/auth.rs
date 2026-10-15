//! Account scenarios: registration, login, JWT validation on REST and QUIC.

use finch_client_core::error::ClientError;
use finch_testbed::client::{connect_with_token, TestClient};
use finch_testbed::server::TestServer;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::Serialize;

#[derive(Serialize)]
struct Claims {
    sub: String,
    username: String,
    role: String,
    exp: usize,
    jti: String,
}

fn now_secs() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize)
        .unwrap_or_default()
}

fn token_for(client: &TestClient, secret: &str, exp: usize) -> String {
    let claims = Claims {
        sub: client.user.id.clone(),
        username: client.user.username.clone(),
        role: "admin".to_string(),
        exp,
        jti: uuid::Uuid::new_v4().to_string(),
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .expect("encode test JWT")
}

async fn assert_token_rejected(server: &TestServer, token: String) -> anyhow::Result<()> {
    let mut api = server.api()?;
    api.set_token(token.clone());
    match api.me().await {
        Err(ClientError::Http { status, .. }) => assert_eq!(status, 401),
        other => panic!("expected HTTP 401, got {other:?}"),
    }
    match connect_with_token(server, &token).await {
        Err(ClientError::AuthRejected { code, .. }) => assert_eq!(code, 401),
        Err(other) => panic!("expected AuthRejected 401 on QUIC, got {other:?}"),
        Ok(_) => panic!("QUIC must reject the token"),
    }
    Ok(())
}

#[tokio::test]
async fn register_and_login() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    assert_eq!(alice.user.username, "alice");

    let api = server.api()?;
    let auth = api.login("alice", "password_123").await?;
    assert_eq!(auth.user.id, alice.user.id);
    Ok(())
}

#[tokio::test]
async fn wrong_password_is_401() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    TestClient::register(&server, "alice").await?;

    let api = server.api()?;
    match api.login("alice", "wrong_password").await {
        Err(ClientError::Http { status, .. }) => assert_eq!(status, 401),
        other => panic!("expected HTTP 401, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn forged_jwt_rejected_on_rest_and_quic() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let forged = token_for(&alice, "not-the-server-secret-but-long-enough!!", now_secs() + 3600);
    assert_token_rejected(&server, forged).await
}

#[tokio::test]
async fn expired_jwt_rejected() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let secret = server.jwt_secret().to_string();
    let expired = token_for(&alice, &secret, now_secs() - 3600);
    assert_token_rejected(&server, expired).await
}

/// The server tells the client its QUIC port, so no port guessing is needed (AUD-028).
#[tokio::test]
async fn login_reports_quic_port() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    TestClient::register(&server, "alice").await?;
    let api = server.api()?;
    let auth = api.login("alice", "password_123").await?;
    assert_eq!(auth.quic_port, Some(server.quic_addr.port()));

    let (addr, host) = finch_client_core::api::resolve_quic_addr_with_port(&server.http_url, auth.quic_port)?;
    assert_eq!(addr, server.quic_addr);
    assert_eq!(host, "127.0.0.1");
    Ok(())
}
