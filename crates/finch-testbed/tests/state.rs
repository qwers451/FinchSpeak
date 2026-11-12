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

#[tokio::test]
async fn state_lists_people_who_are_not_connected_as_offline() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut api = server.api()?;
    // Alice registered but never connected: she is a member all the same.
    let alice = api.register("alice", "password_123", "Alice Display").await?;
    let alice_id = uuid::Uuid::parse_str(&alice.user.id)?;
    let bob = TestClient::register(&server, "bob").await?;
    let state = &bob.session.initial_state;
    assert!(state.online_users.iter().all(|user| user.username != "alice"), "alice is not online");
    let listed = state.offline_users.iter().find(|user| user.username == "alice").expect("alice is listed offline");
    assert_eq!(listed.id, alice_id);
    assert_eq!(listed.display_name, "Alice Display");
    assert_eq!(listed.status, finch_protocol::models::UserStatus::Offline);
    assert!(state.online_users.iter().any(|user| user.id == bob.session.user_id), "the connecting user is online");
    assert!(state.offline_users.iter().all(|user| user.id != bob.session.user_id), "online people are not listed twice");

    api.set_token(alice.token);
    let state = api.server_state().await?;
    assert!(state.offline_users.iter().any(|user| user.id == alice_id), "a REST request does not bring the requester online");
    Ok(())
}

#[tokio::test]
async fn state_omits_banned_users_from_both_lists() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let admin = TestClient::register(&server, "boss").await?;
    let member = server.api()?.register("alice", "password_123", "Alice Display").await?;
    let member_id = uuid::Uuid::parse_str(&member.user.id)?;
    admin.api.ban_user(member_id).await?;

    let state = admin.api.server_state().await?;
    assert!(state.online_users.iter().chain(&state.offline_users).all(|user| user.id != member_id));
    Ok(())
}
