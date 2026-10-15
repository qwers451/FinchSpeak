//! Authoritative state for reconnecting/reloaded frontends and presence refreshes.
use finch_client_core::error::ClientError;
use finch_testbed::client::{connect_with_token, TestClient};
use finch_testbed::server::TestServer;

#[tokio::test]
async fn aud034_initial_state_uses_display_names() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let api = server.api()?;
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
    // No token: rejected. With the token: the live state.
    match server.api()?.server_state().await {
        Err(ClientError::Http { status, .. }) => assert_eq!(status, 401),
        other => panic!("expected 401 without a token, got {other:?}"),
    }
    let state = alice.api.server_state().await?;
    assert!(state.online_users.iter().any(|user| user.id == alice.session.user_id));
    assert!(state.voice_states.iter().any(|voice| voice.peer_id == alice.session.session_peer_id && voice.channel_id == channel));
    Ok(())
}
