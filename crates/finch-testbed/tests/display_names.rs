//! Other people see a user's display name, not their login (live messages and voice rooms).

use std::time::Duration;

use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn live_messages_carry_the_display_name() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let a = TestClient::register_as(&server, "alice", "Alice Cooper", server.quic_addr).await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let (text_id, _) = a.channel(ChannelType::Text);

    a.send(ClientMessage::SendMessage {
        channel_id: text_id,
        content: "hi".into(),
        attachments: vec![],
        client_msg_id: None,
        reply_to: None,
    })
    .await?;

    let author = b
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { author_name, content, .. } if content == "hi" => Some(author_name.clone()),
            _ => None,
        })
        .await?;
    assert_eq!(author, "Alice Cooper");
    Ok(())
}

#[tokio::test]
async fn voice_participants_carry_the_display_name() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register_as(&server, "alice", "Alice Cooper", server.quic_addr).await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let a_peer = a.session.session_peer_id;

    a.join_voice().await?;
    let seen = b
        .expect_control(WAIT, |m| match m {
            ServerMessage::UserJoinedVoice { peer_id, username, .. } if *peer_id == a_peer => Some(username.clone()),
            _ => None,
        })
        .await?;
    assert_eq!(seen, "Alice Cooper");

    // A late joiner gets the same name from the state snapshot.
    let c = TestClient::register(&server, "carol").await?;
    let in_snapshot = c
        .session
        .initial_state
        .voice_states
        .iter()
        .find(|state| state.peer_id == a_peer)
        .map(|state| state.username.clone());
    assert_eq!(in_snapshot.as_deref(), Some("Alice Cooper"));
    Ok(())
}
