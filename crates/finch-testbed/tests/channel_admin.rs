//! Renaming, reordering and deleting channels (admins only), announced to everyone.

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

async fn admin_and_member(server: &TestServer) -> anyhow::Result<(TestClient, TestClient)> {
    Ok((TestClient::register(server, "boss").await?, TestClient::register(server, "alice").await?))
}

#[tokio::test]
async fn rename_is_announced_and_members_cannot_do_it() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, mut alice) = admin_and_member(&server).await?;
    let (text_id, _) = boss.channel(ChannelType::Text);

    let error = alice.api.update_channel(text_id, Some("hacked"), None).await.expect_err("member");
    assert_eq!(status(&error), Some(403));

    boss.api.update_channel(text_id, Some("Town Square"), Some("hello")).await?;
    let renamed = alice
        .expect_control(WAIT, |m| match m {
            ServerMessage::ChannelUpdated { channel } if channel.id == text_id => Some(channel.clone()),
            _ => None,
        })
        .await?;
    assert_eq!((renamed.name.as_str(), renamed.topic.as_deref()), ("town square", Some("hello")));
    let listed = alice.api.list_channels().await?;
    assert!(listed.iter().any(|c| c.id == text_id.to_string() && c.name == "town square"));
    Ok(())
}

#[tokio::test]
async fn bad_names_are_rejected() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, _alice) = admin_and_member(&server).await?;
    let (text_id, _) = boss.channel(ChannelType::Text);
    for name in ["", "   ", &"x".repeat(33)] {
        let error = boss.api.update_channel(text_id, Some(name), None).await.expect_err("bad name");
        assert_eq!(status(&error), Some(400), "name {name:?}");
    }
    assert_eq!(status(&boss.api.update_channel(Uuid::new_v4(), Some("x"), None).await.expect_err("ghost")), Some(404));
    Ok(())
}

#[tokio::test]
async fn reordering_changes_the_list_and_tells_everyone() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, mut alice) = admin_and_member(&server).await?;
    let extra = boss.api.create_channel("extra", "text", None, None).await?;
    let mut ids: Vec<Uuid> = boss.api.list_channels().await?.iter().map(|c| c.id.parse().unwrap()).collect();
    assert!(ids.contains(&extra.id.parse()?));
    ids.reverse();

    assert_eq!(status(&alice.api.reorder_channels(&ids).await.expect_err("member")), Some(403));
    boss.api.reorder_channels(&ids).await?;

    let listed: Vec<Uuid> = alice.api.list_channels().await?.iter().map(|c| c.id.parse().unwrap()).collect();
    assert_eq!(listed, ids);
    alice
        .expect_control(WAIT, |m| matches!(m, ServerMessage::ChannelUpdated { .. }).then_some(()))
        .await?;
    Ok(())
}

#[tokio::test]
async fn deleting_a_text_channel_removes_it_with_its_messages() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, mut alice) = admin_and_member(&server).await?;
    let extra = boss.api.create_channel("extra", "text", None, None).await?;
    let extra_id: Uuid = extra.id.parse()?;
    boss.send(ClientMessage::SendMessage {
        channel_id: extra_id,
        content: "bye".into(),
        attachments: vec![],
        client_msg_id: None,
    })
    .await?;
    alice
        .expect_control(WAIT, |m| matches!(m, ServerMessage::NewMessage { content, .. } if content == "bye").then_some(()))
        .await?;

    assert_eq!(status(&alice.api.delete_channel(extra_id).await.expect_err("member")), Some(403));
    boss.api.delete_channel(extra_id).await?;
    alice
        .expect_control(WAIT, |m| matches!(m, ServerMessage::ChannelDeleted { channel_id } if *channel_id == extra_id).then_some(()))
        .await?;
    assert!(!alice.api.list_channels().await?.iter().any(|c| c.id == extra.id));
    assert_eq!(status(&alice.api.fetch_messages(extra_id, None, 10).await.map(|_| ()).expect_err("gone")), Some(404));
    Ok(())
}

#[tokio::test]
async fn the_last_text_channel_cannot_be_deleted() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, _alice) = admin_and_member(&server).await?;
    let (text_id, _) = boss.channel(ChannelType::Text);
    let error = boss.api.delete_channel(text_id).await.expect_err("last text channel");
    assert_eq!(status(&error), Some(400), "{error}");
    Ok(())
}

#[tokio::test]
async fn deleting_a_voice_channel_sends_its_participants_out_first() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, mut alice) = admin_and_member(&server).await?;
    let (voice_id, _) = alice.join_voice().await?;
    let alice_peer = alice.session.session_peer_id;
    let mut boss = boss;
    boss.expect_control(WAIT, |m| matches!(m, ServerMessage::UserJoinedVoice { peer_id, .. } if *peer_id == alice_peer).then_some(())).await?;

    boss.api.delete_channel(voice_id).await?;
    boss.expect_control(WAIT, |m| matches!(m, ServerMessage::UserLeftVoice { peer_id, .. } if *peer_id == alice_peer).then_some(())).await?;
    alice
        .expect_control(WAIT, |m| matches!(m, ServerMessage::ChannelDeleted { channel_id } if *channel_id == voice_id).then_some(()))
        .await?;

    // Joining the deleted room does nothing: nobody is in it afterwards.
    alice.send(ClientMessage::JoinVoice { channel_id: voice_id }).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let state = boss.api.server_state().await?;
    assert!(state.voice_states.iter().all(|v| v.channel_id != voice_id));
    Ok(())
}
