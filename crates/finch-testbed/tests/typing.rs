//! "Alice is typing...": a hint for the others in the channel, never echoed to the typist.

use std::time::Duration;

use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn others_see_who_is_typing_but_the_typist_does_not() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register_as(&server, "alice", "Alice A", server.quic_addr).await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (channel, _) = alice.channel(ChannelType::Text);

    alice.send(ClientMessage::Typing { channel_id: channel }).await?;
    let (who, name) = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::UserTyping { channel_id, user_id, username } if *channel_id == channel => Some((*user_id, username.clone())),
            _ => None,
        })
        .await?;
    assert_eq!((who, name.as_str()), (alice.session.user_id, "Alice A"));

    alice
        .expect_no_control(Duration::from_millis(400), |m| matches!(m, ServerMessage::UserTyping { .. }))
        .await?;
    Ok(())
}

#[tokio::test]
async fn a_burst_of_typing_events_is_throttled() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (channel, _) = alice.channel(ChannelType::Text);

    for _ in 0..10 {
        alice.send(ClientMessage::Typing { channel_id: channel }).await?;
    }
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::UserTyping { .. }).then_some(())).await?;
    bob.expect_no_control(Duration::from_millis(500), |m| matches!(m, ServerMessage::UserTyping { .. })).await?;
    Ok(())
}

#[tokio::test]
async fn typing_in_a_channel_that_does_not_exist_is_ignored() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    alice.send(ClientMessage::Typing { channel_id: uuid::Uuid::new_v4() }).await?;
    bob.expect_no_control(Duration::from_millis(400), |m| matches!(m, ServerMessage::UserTyping { .. })).await?;
    Ok(())
}
