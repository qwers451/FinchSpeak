//! Registration by invitation: only people with a code from an admin can create an account.

use finch_client_core::error::ClientError;
use finch_testbed::server::TestServer;

const PASSWORD: &str = "Passw0rd!test";

async fn invite_only_server() -> anyhow::Result<TestServer> {
    TestServer::start_with(|config| config.auth.registration = finch_server::config::RegistrationMode::Invite).await
}

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

/// The server's first user is the admin and registers without a code; returns their client.
async fn admin(server: &TestServer) -> anyhow::Result<finch_client_core::api::ApiClient> {
    let mut api = server.api()?;
    let auth = api.register("boss", PASSWORD, "Boss").await?;
    api.set_token(auth.token);
    Ok(api)
}

#[tokio::test]
async fn registration_needs_an_invite_once_the_server_has_users() -> anyhow::Result<()> {
    let server = invite_only_server().await?;
    let _admin = admin(&server).await?;

    let error = server.api()?.register("alice", PASSWORD, "Alice").await.expect_err("no code");
    assert_eq!(status(&error), Some(403), "{error}");
    let error = server.api()?.register_with_invite("alice", PASSWORD, "Alice", "NOSUCHCODE").await.expect_err("bad code");
    assert_eq!(status(&error), Some(403), "{error}");
    Ok(())
}

#[tokio::test]
async fn an_invite_is_used_up_after_its_uses() -> anyhow::Result<()> {
    let server = invite_only_server().await?;
    let admin = admin(&server).await?;
    let invite = admin.create_invite(Some(1), None).await?;

    let alice = server.api()?.register_with_invite("alice", PASSWORD, "Alice", &invite.code).await?;
    assert_eq!(alice.user.role, "member");
    let error = server.api()?.register_with_invite("bob", PASSWORD, "Bob", &invite.code).await.expect_err("used up");
    assert_eq!(status(&error), Some(403), "{error}");

    let listed = admin.list_invites().await?;
    assert_eq!(listed.iter().find(|i| i.code == invite.code).map(|i| i.uses), Some(1));
    Ok(())
}

#[tokio::test]
async fn an_invite_can_allow_several_registrations() -> anyhow::Result<()> {
    let server = invite_only_server().await?;
    let admin = admin(&server).await?;
    let invite = admin.create_invite(Some(2), None).await?;
    for name in ["alice", "bob"] {
        server.api()?.register_with_invite(name, PASSWORD, name, &invite.code).await?;
    }
    let error = server.api()?.register_with_invite("carol", PASSWORD, "Carol", &invite.code).await.expect_err("third use");
    assert_eq!(status(&error), Some(403), "{error}");
    Ok(())
}

#[tokio::test]
async fn an_expired_invite_does_not_work() -> anyhow::Result<()> {
    let server = invite_only_server().await?;
    let admin = admin(&server).await?;
    let invite = admin.create_invite(Some(5), Some(0)).await?;
    let error = server.api()?.register_with_invite("alice", PASSWORD, "Alice", &invite.code).await.expect_err("expired");
    assert_eq!(status(&error), Some(403), "{error}");
    Ok(())
}

#[tokio::test]
async fn a_failed_registration_does_not_burn_the_invite() -> anyhow::Result<()> {
    let server = invite_only_server().await?;
    let admin = admin(&server).await?;
    let invite = admin.create_invite(Some(1), None).await?;

    // The name "boss" is taken: the registration fails, the code must stay usable.
    let error = server.api()?.register_with_invite("boss", PASSWORD, "Impostor", &invite.code).await.expect_err("taken");
    assert_eq!(status(&error), Some(409), "{error}");
    server.api()?.register_with_invite("alice", PASSWORD, "Alice", &invite.code).await?;
    Ok(())
}

#[tokio::test]
async fn only_admins_manage_invites_and_revoked_ones_stop_working() -> anyhow::Result<()> {
    let server = invite_only_server().await?;
    let admin = admin(&server).await?;
    let invite = admin.create_invite(Some(3), None).await?;

    let mut alice = server.api()?;
    let auth = alice.register_with_invite("alice", PASSWORD, "Alice", &invite.code).await?;
    alice.set_token(auth.token);
    assert_eq!(status(&alice.create_invite(Some(1), None).await.expect_err("member")), Some(403));
    assert_eq!(status(&alice.list_invites().await.expect_err("member")), Some(403));

    admin.revoke_invite(&invite.code).await?;
    let error = server.api()?.register_with_invite("bob", PASSWORD, "Bob", &invite.code).await.expect_err("revoked");
    assert_eq!(status(&error), Some(403), "{error}");
    Ok(())
}

#[tokio::test]
async fn open_registration_still_works_when_the_operator_chooses_it() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _admin = admin(&server).await?;
    server.api()?.register("alice", PASSWORD, "Alice").await?;
    Ok(())
}
