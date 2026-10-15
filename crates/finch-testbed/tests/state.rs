//! Authoritative state for reconnecting/reloaded frontends and presence refreshes.
use finch_client_core::api::ApiClient;
use finch_testbed::client::{connect_with_token, TestClient};
use finch_testbed::server::TestServer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn get_state(server: &TestServer, token: Option<&str>) -> anyhow::Result<String> {
    let mut stream = tokio::net::TcpStream::connect(server.http_url.trim_start_matches("http://")).await?;
    let authorization = token.map(|token| format!("Authorization: Bearer {token}\r\n")).unwrap_or_default();
    stream.write_all(format!("GET /api/state HTTP/1.0\r\nHost: localhost\r\n{authorization}Connection: close\r\n\r\n").as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    Ok(String::from_utf8(response)?)
}

#[tokio::test]
async fn aud034_initial_state_uses_display_names() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let api = ApiClient::new(&server.http_url)?;
    let alice = api.register("alice", "password_123", "Alice Display").await?;
    let (_alice, _events) = connect_with_token(&server, &alice.token).await?;
    let bob = TestClient::register(&server, "bob").await?;
    let profile = bob.session.initial_state.online_users.iter().find(|user| user.username == "alice").unwrap();
    assert_eq!(profile.display_name, "Alice Display");
    Ok(())
}

#[tokio::test]
async fn authenticated_state_endpoint_returns_current_voice_and_users() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.join_voice().await?;
    assert_eq!(get_state(&server, None).await?.split_whitespace().nth(1), Some("401"));
    let response = get_state(&server, alice.api.token()).await?;
    assert_eq!(response.split_whitespace().nth(1), Some("200"));
    let state: finch_protocol::models::ServerState = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1)?;
    assert!(state.online_users.iter().any(|user| user.id == alice.session.user_id));
    assert!(state.voice_states.iter().any(|voice| voice.peer_id == alice.session.session_peer_id && voice.channel_id == channel));
    Ok(())
}
