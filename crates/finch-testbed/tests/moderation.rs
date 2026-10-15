//! Admin tools: list members, change roles, kick and ban.

use std::time::Duration;

use finch_client_core::api::ApiClient;
use finch_client_core::error::ClientError;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(3);

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

/// Admin ("boss", the first account) and a member ("alice") with live sessions.
async fn admin_and_member(server: &TestServer) -> anyhow::Result<(TestClient, TestClient)> {
    let boss = TestClient::register(server, "boss").await?;
    let alice = TestClient::register(server, "alice").await?;
    Ok((boss, alice))
}

async fn wait_until_disconnected(client: &mut TestClient) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + WAIT;
    while tokio::time::Instant::now() < deadline {
        let _ = client.next_event(Duration::from_millis(200)).await;
        if client.session.connection.close_reason().is_some() {
            return Ok(());
        }
    }
    anyhow::bail!("the client was not disconnected")
}

#[tokio::test]
async fn members_cannot_use_admin_tools() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, alice) = admin_and_member(&server).await?;
    let boss_id = boss.session.user_id;

    assert_eq!(status(&alice.api.admin_users().await.expect_err("list")), Some(403));
    assert_eq!(status(&alice.api.ban_user(boss_id).await.expect_err("ban")), Some(403));
    assert_eq!(status(&alice.api.kick_user(boss_id).await.expect_err("kick")), Some(403));
    assert_eq!(status(&alice.api.set_user_role(boss_id, "member").await.expect_err("role")), Some(403));
    Ok(())
}

#[tokio::test]
async fn admin_sees_everyone_with_role_and_state() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, alice) = admin_and_member(&server).await?;
    let users = boss.api.admin_users().await?;
    let find = |name: &str| users.iter().find(|u| u.username == name).cloned();
    let (b, a) = (find("boss").expect("boss"), find("alice").expect("alice"));
    assert_eq!((b.role.as_str(), b.banned, b.online), ("admin", false, true));
    assert_eq!((a.role.as_str(), a.banned, a.online), ("member", false, true));
    assert_eq!(a.id, alice.session.user_id.to_string());
    Ok(())
}

#[tokio::test]
async fn ban_disconnects_revokes_tokens_and_blocks_login_until_unbanned() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, mut alice) = admin_and_member(&server).await?;
    let alice_id = alice.session.user_id;

    boss.api.ban_user(alice_id).await?;
    wait_until_disconnected(&mut alice).await?;
    assert_eq!(status(&alice.api.me().await.expect_err("token revoked")), Some(401));
    let error = server.api()?.login("alice", finch_testbed::client::TEST_PASSWORD).await.expect_err("banned");
    assert_eq!(status(&error), Some(403), "{error}");
    assert!(boss.api.admin_users().await?.iter().any(|u| u.username == "alice" && u.banned));

    boss.api.unban_user(alice_id).await?;
    server.api()?.login("alice", finch_testbed::client::TEST_PASSWORD).await?;
    Ok(())
}

#[tokio::test]
async fn kick_disconnects_but_does_not_stop_a_new_login() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, mut alice) = admin_and_member(&server).await?;
    boss.api.kick_user(alice.session.user_id).await?;
    wait_until_disconnected(&mut alice).await?;
    assert_eq!(status(&alice.api.me().await.expect_err("token revoked")), Some(401));
    server.api()?.login("alice", finch_testbed::client::TEST_PASSWORD).await?;
    Ok(())
}

#[tokio::test]
async fn roles_can_change_but_the_last_admin_stays_and_admins_are_not_banned() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, alice) = admin_and_member(&server).await?;
    let (boss_id, alice_id) = (boss.session.user_id, alice.session.user_id);

    // Cannot remove the only admin, nor ban or kick yourself.
    assert_eq!(status(&boss.api.set_user_role(boss_id, "member").await.expect_err("last admin")), Some(400));
    assert_eq!(status(&boss.api.ban_user(boss_id).await.expect_err("self ban")), Some(400));
    assert_eq!(status(&boss.api.kick_user(boss_id).await.expect_err("self kick")), Some(400));
    assert_eq!(status(&boss.api.set_user_role(alice_id, "owner").await.expect_err("bad role")), Some(400));

    // Promote alice: her next request can create invites and she is protected from a ban.
    boss.api.set_user_role(alice_id, "admin").await?;
    alice.api.create_invite(Some(1), None).await?;
    assert_eq!(status(&boss.api.ban_user(alice_id).await.expect_err("admin ban")), Some(400));

    // Now boss can step down, since alice is an admin too.
    boss.api.set_user_role(boss_id, "member").await?;
    assert_eq!(status(&boss.api.create_invite(Some(1), None).await.expect_err("demoted")), Some(403));
    Ok(())
}

#[tokio::test]
async fn unknown_users_are_reported_as_not_found() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let (boss, _alice) = admin_and_member(&server).await?;
    let ghost = uuid::Uuid::new_v4();
    assert_eq!(status(&boss.api.ban_user(ghost).await.expect_err("ghost")), Some(404));
    let _: &ApiClient = &boss.api;
    Ok(())
}
