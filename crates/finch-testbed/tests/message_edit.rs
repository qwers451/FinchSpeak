//! Editing and deleting chat messages.

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

/// `author` posts "original"; returns (channel, message id) once `watcher` has seen it.
async fn post(author: &TestClient, watcher: &mut TestClient) -> anyhow::Result<(Uuid, Uuid)> {
    let (channel, _) = author.channel(ChannelType::Text);
    author
        .send(ClientMessage::SendMessage { channel_id: channel, content: "original".into(), attachments: vec![], client_msg_id: None })
        .await?;
    let id = watcher
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { message_id, content, .. } if content == "original" => Some(*message_id),
            _ => None,
        })
        .await?;
    Ok((channel, id))
}

#[tokio::test]
async fn the_author_can_edit_and_everyone_sees_it() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (channel, id) = post(&alice, &mut bob).await?;

    alice.api.edit_message(id, "fixed").await?;
    let edited = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::MessageEdited { message_id, content, edited_at, .. } if *message_id == id => Some((content.clone(), *edited_at)),
            _ => None,
        })
        .await?;
    assert_eq!(edited.0, "fixed");

    let history = bob.api.fetch_messages_page(channel, None, 10).await?;
    let message = history.messages.iter().find(|m| m.id == id).expect("message in history");
    assert_eq!(message.content, "fixed");
    assert_eq!(message.edited_at, Some(edited.1));

    // Not even an admin edits somebody else's words.
    assert_eq!(status(&boss.api.edit_message(id, "mine now").await.expect_err("admin edit")), Some(403));
    assert_eq!(status(&bob.api.edit_message(id, "mine now").await.expect_err("other edit")), Some(403));
    let _ = alice.next_event(Duration::from_millis(10)).await;
    Ok(())
}

#[tokio::test]
async fn bad_edits_are_refused() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (_, id) = post(&alice, &mut bob).await?;
    for content in ["", "   ", &"x".repeat(4001)] {
        let error = alice.api.edit_message(id, content).await.expect_err("bad content");
        assert_eq!(status(&error), Some(400), "{:?}", content.len());
    }
    assert_eq!(status(&alice.api.edit_message(Uuid::new_v4(), "x").await.expect_err("ghost")), Some(404));
    let _ = alice.next_event(Duration::from_millis(10)).await;
    Ok(())
}

#[tokio::test]
async fn author_and_admin_can_delete_but_others_cannot() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;

    let (channel, first) = post(&alice, &mut bob).await?;
    assert_eq!(status(&bob.api.delete_message(first).await.expect_err("other")), Some(403));
    alice.api.delete_message(first).await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::MessageDeleted { message_id, .. } if *message_id == first).then_some(())).await?;
    assert!(bob.api.fetch_messages_page(channel, None, 10).await?.messages.iter().all(|m| m.id != first));

    let (_, second) = post(&alice, &mut bob).await?;
    boss.api.delete_message(second).await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::MessageDeleted { message_id, .. } if *message_id == second).then_some(())).await?;
    assert_eq!(status(&alice.api.delete_message(second).await.expect_err("already gone")), Some(404));
    let _ = alice.next_event(Duration::from_millis(10)).await;
    Ok(())
}
