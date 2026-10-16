//! Changing your own display name and password.

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::{TestClient, TEST_PASSWORD};
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(3);

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

#[tokio::test]
async fn a_new_display_name_reaches_everyone_and_later_messages() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;

    let me = alice.api.update_profile("  Alice Cooper  ").await?;
    assert_eq!(me.display_name, "Alice Cooper");
    let seen = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::UserProfileChanged { user_id, display_name } => Some((*user_id, display_name.clone())),
            _ => None,
        })
        .await?;
    assert_eq!(seen, (alice.session.user_id, "Alice Cooper".to_string()));
    assert_eq!(alice.api.me().await?.display_name, "Alice Cooper");

    // Messages and voice presence use the new name without reconnecting.
    let (channel, _) = alice.channel(ChannelType::Text);
    alice
        .send(ClientMessage::SendMessage { channel_id: channel, content: "hi".into(), attachments: vec![], client_msg_id: None, reply_to: None })
        .await?;
    let author = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { author_name, .. } => Some(author_name.clone()),
            _ => None,
        })
        .await?;
    assert_eq!(author, "Alice Cooper");
    let _ = alice.next_event(Duration::from_millis(10)).await;
    Ok(())
}

#[tokio::test]
async fn bad_display_names_are_refused() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    for name in ["", "   ", &"x".repeat(33)] {
        assert_eq!(status(&alice.api.update_profile(name).await.expect_err("bad name")), Some(400));
    }
    Ok(())
}

#[tokio::test]
async fn changing_the_password_signs_out_the_other_devices() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    // A second device of the same account.
    let other = server.api()?.login("alice", TEST_PASSWORD).await?;
    let mut other_api = server.api()?;
    other_api.set_token(other.token);
    other_api.me().await?;

    alice.api.change_password(TEST_PASSWORD, "a-better-pass-9").await?;

    assert_eq!(status(&other_api.me().await.expect_err("other device")), Some(401), "other devices are signed out");
    alice.api.me().await?; // the device that changed it stays signed in
    assert_eq!(status(&server.api()?.login("alice", TEST_PASSWORD).await.expect_err("old password")), Some(401));
    server.api()?.login("alice", "a-better-pass-9").await?;
    Ok(())
}

#[tokio::test]
async fn the_current_password_is_required_and_the_new_one_must_be_decent() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    assert_eq!(status(&alice.api.change_password("not-it", "a-better-pass-9").await.expect_err("wrong")), Some(403));
    assert_eq!(status(&alice.api.change_password(TEST_PASSWORD, "short").await.expect_err("short")), Some(400));
    server.api()?.login("alice", TEST_PASSWORD).await?; // nothing changed
    Ok(())
}
