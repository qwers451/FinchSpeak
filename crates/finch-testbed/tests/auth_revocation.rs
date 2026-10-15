use std::time::Duration;
use finch_client_core::error::ClientError;
use finch_testbed::client::{connect_with_token, TestClient, TEST_PASSWORD};
use finch_testbed::server::TestServer;

#[tokio::test]
async fn aud039_logout_revokes_rest_and_quic_without_revoking_a_second_login() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let token = alice.api.token().unwrap().to_string();
    let mut second_api = server.api()?;
    let second_login = second_api.login("alice", TEST_PASSWORD).await?;
    second_api.set_token(second_login.token.clone());
    let (_second_session, _events) = connect_with_token(&server, &second_login.token).await?;
    alice.api.logout().await?;
    tokio::time::timeout(Duration::from_secs(3), alice.session.connection.closed()).await?;
    assert!(matches!(alice.api.me().await, Err(ClientError::Http { status: 401, .. })));
    assert!(matches!(connect_with_token(&server, &token).await, Err(ClientError::AuthRejected { .. })));
    assert_eq!(second_api.me().await?.id, alice.user.id);
    Ok(())
}
