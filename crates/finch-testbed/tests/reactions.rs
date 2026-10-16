//! Emoji reactions on messages.

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(3);

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

async fn post(author: &TestClient, watcher: &mut TestClient) -> anyhow::Result<(Uuid, Uuid)> {
    let (channel, _) = author.channel(ChannelType::Text);
    author
        .send(ClientMessage::SendMessage { channel_id: channel, content: "react to me".into(), attachments: vec![], client_msg_id: None, reply_to: None })
        .await?;
    let id = watcher
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { message_id, .. } => Some(*message_id),
            _ => None,
        })
        .await?;
    Ok((channel, id))
}

#[tokio::test]
async fn reactions_are_announced_and_kept_in_history() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (channel, id) = post(&alice, &mut bob).await?;

    bob.api.add_reaction(id, "👍").await?;
    let seen = alice
        .expect_control(WAIT, |m| match m {
            ServerMessage::ReactionChanged { message_id, user_id, emoji, added, .. } if *message_id == id => Some((*user_id, emoji.clone(), *added)),
            _ => None,
        })
        .await?;
    assert_eq!(seen, (bob.session.user_id, "👍".to_string(), true));

    alice.api.add_reaction(id, "👍").await?;
    alice.api.add_reaction(id, "🎉").await?;
    let history = bob.api.fetch_messages_page(channel, None, 10).await?;
    let reactions = &history.messages.iter().find(|m| m.id == id).expect("message").reactions;
    let thumbs = reactions.iter().find(|r| r.emoji == "👍").expect("thumbs up");
    assert_eq!(thumbs.user_ids.len(), 2);
    assert_eq!(reactions.iter().find(|r| r.emoji == "🎉").map(|r| r.user_ids.len()), Some(1));

    bob.api.remove_reaction(id, "👍").await?;
    alice
        .expect_control(WAIT, |m| matches!(m, ServerMessage::ReactionChanged { added: false, emoji, .. } if emoji == "👍").then_some(()))
        .await?;
    let history = alice.api.fetch_messages_page(channel, None, 10).await?;
    let thumbs = history.messages.iter().find(|m| m.id == id).and_then(|m| m.reactions.iter().find(|r| r.emoji == "👍").cloned());
    assert_eq!(thumbs.map(|r| r.user_ids), Some(vec![alice.session.user_id]));
    Ok(())
}

#[tokio::test]
async fn adding_twice_is_harmless_and_bad_input_is_refused() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (channel, id) = post(&alice, &mut bob).await?;

    bob.api.add_reaction(id, "👍").await?;
    bob.api.add_reaction(id, "👍").await?;
    let history = bob.api.fetch_messages_page(channel, None, 10).await?;
    assert_eq!(history.messages[0].reactions[0].user_ids.len(), 1);

    assert_eq!(status(&bob.api.add_reaction(id, &"x".repeat(40)).await.expect_err("too long")), Some(400));
    assert_eq!(status(&bob.api.add_reaction(Uuid::new_v4(), "👍").await.expect_err("ghost")), Some(404));
    // Removing what is not there is not an error.
    bob.api.remove_reaction(id, "🎉").await?;
    Ok(())
}
