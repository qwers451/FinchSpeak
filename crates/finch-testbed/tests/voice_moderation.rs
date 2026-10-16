//! Moderators send people out of a voice room or silence them there (stage 7, M5).

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::ServerMessage;
use finch_testbed::client::{audio_datagram, TestClient};
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(3);

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

async fn room_with_staff(server: &TestServer) -> anyhow::Result<(TestClient, TestClient, TestClient, TestClient, u32)> {
    let boss = TestClient::register(server, "boss").await?;
    let moderator = TestClient::register(server, "moddy").await?;
    let mut alice = TestClient::register(server, "alice").await?;
    let mut bob = TestClient::register(server, "bob").await?;
    boss.api.set_user_role(moderator.session.user_id, "moderator").await?;
    let (_, short) = alice.join_voice().await?;
    bob.join_voice().await?;
    Ok((boss, moderator, alice, bob, short))
}

#[tokio::test]
async fn kicking_from_voice_removes_the_person_and_tells_everyone() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (_boss, moderator, mut alice, mut bob, _) = room_with_staff(&server).await?;
    let alice_peer = alice.session.session_peer_id;

    moderator.api.moderate_voice(alice.session.user_id, "kick").await?;

    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::UserLeftVoice { peer_id, .. } if *peer_id == alice_peer).then_some(())).await?;
    alice.expect_control(WAIT, |m| matches!(m, ServerMessage::VoiceKicked { .. }).then_some(())).await?;
    // She stays connected to the server and may join again.
    assert!(alice.session.connection.close_reason().is_none());
    alice.join_voice().await?;
    Ok(())
}

#[tokio::test]
async fn a_server_muted_person_is_not_heard_and_cannot_undo_it() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (_boss, moderator, mut alice, mut bob, short) = room_with_staff(&server).await?;
    let alice_peer = alice.session.session_peer_id;

    moderator.api.moderate_voice(alice.session.user_id, "mute").await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::ServerMuteChanged { peer_id, server_muted: true } if *peer_id == alice_peer).then_some(())).await?;
    alice.expect_control(WAIT, |m| matches!(m, ServerMessage::ServerMuteChanged { server_muted: true, .. }).then_some(())).await?;

    bob.drain_datagrams();
    for seq in 0..30u16 {
        alice.session.send_datagram(audio_datagram(short, alice_peer, seq, 60))?;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(bob.drain_datagrams().is_empty(), "a server-muted voice is not forwarded");

    // Her own "unmuted" does not help.
    alice.send(finch_protocol::messages::ClientMessage::UpdateVoiceState { muted: false, deafened: false }).await?;
    for seq in 30..60u16 {
        alice.session.send_datagram(audio_datagram(short, alice_peer, seq, 60))?;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(bob.drain_datagrams().is_empty());

    // Lifting it brings the voice back.
    moderator.api.moderate_voice(alice.session.user_id, "unmute").await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::ServerMuteChanged { server_muted: false, .. }).then_some(())).await?;
    bob.drain_datagrams();
    for seq in 60..100u16 {
        alice.session.send_datagram(audio_datagram(short, alice_peer, seq, 60))?;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(bob.drain_datagrams().len() >= 30);
    Ok(())
}

#[tokio::test]
async fn late_joiners_see_who_is_server_muted() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (_boss, moderator, alice, _bob, _) = room_with_staff(&server).await?;
    moderator.api.moderate_voice(alice.session.user_id, "mute").await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let carol = TestClient::register(&server, "carol").await?;
    let state = carol.api.server_state().await?;
    let flag = state.voice_states.iter().find(|v| v.user_id == alice.session.user_id).map(|v| v.server_muted);
    assert_eq!(flag, Some(true));
    Ok(())
}

#[tokio::test]
async fn only_staff_may_moderate_voice_and_only_members() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, moderator, alice, bob, _) = room_with_staff(&server).await?;
    assert_eq!(status(&alice.api.moderate_voice(bob.session.user_id, "kick").await.expect_err("member")), Some(403));
    assert_eq!(status(&moderator.api.moderate_voice(boss.session.user_id, "mute").await.expect_err("admin")), Some(403));
    assert_eq!(status(&boss.api.moderate_voice(alice.session.user_id, "dance").await.expect_err("action")), Some(400));
    assert_eq!(status(&boss.api.moderate_voice(boss.session.user_id, "kick").await.expect_err("self")), Some(400));
    Ok(())
}

#[tokio::test]
async fn a_person_who_is_not_in_a_room_cannot_be_moderated_there() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, _moderator, _alice, _bob, _) = room_with_staff(&server).await?;
    let carol = TestClient::register(&server, "carol").await?;
    assert_eq!(status(&boss.api.moderate_voice(carol.session.user_id, "kick").await.expect_err("not in voice")), Some(400));
    Ok(())
}

#[tokio::test]
async fn voice_moderation_is_logged() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, moderator, alice, bob, _) = room_with_staff(&server).await?;
    moderator.api.moderate_voice(alice.session.user_id, "mute").await?;
    boss.api.moderate_voice(bob.session.user_id, "kick").await?;
    let log = boss.api.audit_log(None, 10).await?;
    assert!(log.iter().any(|e| e.action == "voice_mute" && e.target.as_deref() == Some("alice") && e.actor_name == "moddy"));
    assert!(log.iter().any(|e| e.action == "voice_kick" && e.target.as_deref() == Some("bob")));
    Ok(())
}
