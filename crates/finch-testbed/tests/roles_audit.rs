//! Moderators, timed bans, the action log, runtime registration mode and the server page (stage 7, M5).

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::{TestClient, TEST_PASSWORD};
use finch_testbed::server::TestServer;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(3);

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

/// boss (admin), mod (moderator), alice and bob (members).
async fn crew(server: &TestServer) -> anyhow::Result<(TestClient, TestClient, TestClient, TestClient)> {
    let boss = TestClient::register(server, "boss").await?;
    let moderator = TestClient::register(server, "moddy").await?;
    let alice = TestClient::register(server, "alice").await?;
    let bob = TestClient::register(server, "bob").await?;
    boss.api.set_user_role(moderator.session.user_id, "moderator").await?;
    Ok((boss, moderator, alice, bob))
}

#[tokio::test]
async fn a_moderator_can_look_and_kick_members_but_not_manage() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, moderator, alice, bob) = crew(&server).await?;

    let users = moderator.api.admin_users().await?;
    assert_eq!(users.iter().find(|u| u.username == "moddy").unwrap().role, "moderator");

    moderator.api.kick_user(alice.session.user_id).await?;
    // Not theirs to do:
    assert_eq!(status(&moderator.api.ban_user(bob.session.user_id).await.expect_err("ban")), Some(403));
    assert_eq!(status(&moderator.api.set_user_role(bob.session.user_id, "admin").await.expect_err("role")), Some(403));
    assert_eq!(status(&moderator.api.create_channel("secret", "text", None, None).await.expect_err("channel")), Some(403));
    assert_eq!(status(&moderator.api.create_invite(None, None).await.expect_err("invite")), Some(403));
    assert_eq!(status(&moderator.api.server_overview().await.expect_err("server page")), Some(403));
    assert_eq!(status(&moderator.api.set_registration("open").await.expect_err("registration")), Some(403));
    // Nor may they act on staff.
    assert_eq!(status(&moderator.api.kick_user(boss.session.user_id).await.expect_err("kick admin")), Some(403));
    Ok(())
}

#[tokio::test]
async fn members_have_no_staff_powers_and_demotion_takes_effect_at_once() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, moderator, alice, _bob) = crew(&server).await?;
    assert_eq!(status(&alice.api.audit_log(None, 10).await.expect_err("audit")), Some(403));
    assert!(moderator.api.audit_log(None, 10).await.is_ok());

    boss.api.set_user_role(moderator.session.user_id, "member").await?;
    assert_eq!(status(&moderator.api.audit_log(None, 10).await.expect_err("demoted")), Some(403));
    assert_eq!(status(&moderator.api.kick_user(alice.session.user_id).await.expect_err("demoted")), Some(403));
    Ok(())
}

#[tokio::test]
async fn the_last_admin_cannot_be_demoted_even_to_moderator() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    assert_eq!(status(&boss.api.set_user_role(boss.session.user_id, "moderator").await.expect_err("last admin")), Some(400));
    assert_eq!(status(&boss.api.set_user_role(boss.session.user_id, "king").await.expect_err("unknown role")), Some(400));
    Ok(())
}

#[tokio::test]
async fn moderators_delete_other_peoples_messages_and_it_is_logged() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (_boss, moderator, mut alice, _bob) = crew(&server).await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    alice
        .send(ClientMessage::SendMessage { channel_id: channel, content: "rude".into(), attachments: vec![], client_msg_id: None, reply_to: None })
        .await?;
    let message: Uuid = alice
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { message_id, .. } => Some(*message_id),
            _ => None,
        })
        .await?;
    moderator.api.delete_message(message).await?;
    let log = moderator.api.audit_log(None, 10).await?;
    assert!(log.iter().any(|e| e.action == "message_deleted" && e.actor_name == "moddy"), "{log:?}");
    Ok(())
}

#[tokio::test]
async fn every_administrative_action_leaves_an_entry_newest_first_and_pages() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, _moderator, alice, bob) = crew(&server).await?;
    boss.api.kick_user(alice.session.user_id).await?;
    boss.api.ban_user(bob.session.user_id).await?;
    boss.api.unban_user(bob.session.user_id).await?;
    let invite = boss.api.create_invite(Some(3), Some(24)).await?;
    boss.api.revoke_invite(&invite.code).await?;
    let channel = boss.api.create_channel("extra", "text", None, None).await?;
    boss.api.delete_channel(Uuid::parse_str(&channel.id)?).await?;
    boss.api.set_registration("open").await?;

    let log = boss.api.audit_log(None, 50).await?;
    let actions: Vec<&str> = log.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        ["registration_changed", "channel_deleted", "channel_created", "invite_revoked", "invite_created", "unban", "ban", "kick", "role_changed"],
        "newest first"
    );
    assert!(log.iter().all(|e| e.actor_name == "boss"));
    assert_eq!(log.iter().find(|e| e.action == "kick").unwrap().target.as_deref(), Some("alice"));

    // Paging by id walks back without repeats.
    let first = boss.api.audit_log(None, 4).await?;
    let second = boss.api.audit_log(Some(first.last().unwrap().id), 50).await?;
    assert_eq!(first.len() + second.len(), log.len());
    assert!(second.iter().all(|e| first.iter().all(|f| f.id != e.id)));
    Ok(())
}

#[tokio::test]
async fn a_timed_ban_blocks_the_login_and_lifts_itself() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, _moderator, _alice, bob) = crew(&server).await?;
    boss.api.ban_user_for(bob.session.user_id, Some(2)).await?;

    let users = boss.api.admin_users().await?;
    let listed = users.iter().find(|u| u.username == "bob").unwrap();
    assert!(listed.banned && listed.banned_until.is_some());
    let api = server.api()?;
    assert_eq!(status(&api.login("bob", TEST_PASSWORD).await.expect_err("banned")), Some(403));

    tokio::time::sleep(Duration::from_millis(2600)).await;
    api.login("bob", TEST_PASSWORD).await?;
    let after = boss.api.admin_users().await?;
    assert!(!after.iter().find(|u| u.username == "bob").unwrap().banned);
    Ok(())
}

#[tokio::test]
async fn a_permanent_ban_stays_until_someone_lifts_it() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, _moderator, _alice, bob) = crew(&server).await?;
    boss.api.ban_user(bob.session.user_id).await?;
    let api = server.api()?;
    assert_eq!(status(&api.login("bob", TEST_PASSWORD).await.expect_err("banned")), Some(403));
    boss.api.unban_user(bob.session.user_id).await?;
    api.login("bob", TEST_PASSWORD).await?;
    Ok(())
}

#[tokio::test]
async fn the_admin_switches_registration_without_a_restart_and_sees_the_server_page() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| c.auth.registration = finch_server::config::RegistrationMode::Open).await?;
    let boss = TestClient::register(&server, "boss").await?;
    let overview = boss.api.server_overview().await?;
    assert_eq!(overview.registration, "open");
    assert!(overview.certificate_fingerprint.as_deref().is_some_and(|f| f.contains(':')));
    assert_eq!(overview.version, env!("CARGO_PKG_VERSION"));

    boss.api.set_registration("invite").await?;
    let api = server.api()?;
    let refused = api.register("newcomer", TEST_PASSWORD, "Newcomer").await.expect_err("needs an invite");
    assert_eq!(status(&refused), Some(403));
    let invite = boss.api.create_invite(Some(1), Some(1)).await?;
    api.register_with_invite("newcomer", TEST_PASSWORD, "Newcomer", &invite.code).await?;
    assert_eq!(boss.api.server_overview().await?.registration, "invite");
    assert_eq!(status(&boss.api.set_registration("closed").await.expect_err("bad mode")), Some(400));
    Ok(())
}

#[tokio::test]
async fn a_backup_can_be_made_from_the_server_page() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    assert_eq!(boss.api.server_overview().await?.backups_count, 0);
    boss.api.backup_now().await?;
    let overview = boss.api.server_overview().await?;
    assert_eq!(overview.backups_count, 1);
    assert!(overview.last_backup_at.is_some() && overview.last_backup_bytes.unwrap_or(0) > 0);
    Ok(())
}
