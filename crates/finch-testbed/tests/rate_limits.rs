//! Guessing passwords, mass registration and message floods are slowed down (stage 7, M5).

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::{TestClient, TEST_PASSWORD};
use finch_testbed::server::TestServer;
use uuid::Uuid;

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

#[tokio::test]
async fn five_wrong_passwords_lock_that_login_for_a_while_and_the_right_one_works_after() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| c.auth.login_lockout_secs = 2).await?;
    let _alice = TestClient::register(&server, "alice").await?;
    let _bob = TestClient::register(&server, "bob").await?;
    let api = server.api()?;

    for attempt in 1..=4 {
        assert_eq!(status(&api.login("alice", "wrong-password").await.expect_err("wrong")), Some(401), "attempt {attempt}");
    }
    // The fifth wrong one locks it.
    assert_eq!(status(&api.login("alice", "wrong-password").await.expect_err("locked")), Some(429));
    // Even the right password waits.
    assert_eq!(status(&api.login("alice", TEST_PASSWORD).await.expect_err("locked")), Some(429));
    // Somebody else is not affected.
    api.login("bob", TEST_PASSWORD).await?;

    tokio::time::sleep(Duration::from_millis(2500)).await;
    api.login("alice", TEST_PASSWORD).await?;
    Ok(())
}

#[tokio::test]
async fn a_correct_password_resets_the_count() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _alice = TestClient::register(&server, "alice").await?;
    let api = server.api()?;
    for _ in 0..4 {
        assert_eq!(status(&api.login("alice", "nope").await.expect_err("wrong")), Some(401));
    }
    api.login("alice", TEST_PASSWORD).await?;
    for _ in 0..4 {
        assert_eq!(status(&api.login("alice", "nope").await.expect_err("wrong")), Some(401));
    }
    api.login("alice", TEST_PASSWORD).await?;
    Ok(())
}

#[tokio::test]
async fn unknown_logins_are_slowed_down_the_same_way() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let api = server.api()?;
    for _ in 0..4 {
        assert_eq!(status(&api.login("nobody", "x").await.expect_err("unknown")), Some(401));
    }
    assert_eq!(status(&api.login("nobody", "x").await.expect_err("locked")), Some(429));
    Ok(())
}

#[tokio::test]
async fn too_many_registrations_from_one_address_are_refused() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| c.auth.register_per_hour = 3).await?;
    for n in 0..3 {
        let api = server.api()?;
        api.register(&format!("user{n}"), TEST_PASSWORD, "User").await?;
    }
    let api = server.api()?;
    assert_eq!(status(&api.register("user3", TEST_PASSWORD, "User").await.expect_err("limit")), Some(429));
    Ok(())
}

#[tokio::test]
async fn a_message_flood_is_rejected_with_429_and_normal_chat_goes_on() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| {
        c.auth.message_limit = 5;
        c.auth.message_window_secs = 2;
    })
    .await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);

    let mut ids = Vec::new();
    for i in 0..9 {
        let id = Uuid::new_v4();
        ids.push(id);
        alice
            .send(ClientMessage::SendMessage { channel_id: channel, content: format!("spam {i}"), attachments: vec![], client_msg_id: Some(id), reply_to: None })
            .await?;
    }
    let mut accepted = 0;
    let mut rejected = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while accepted + rejected < 9 && tokio::time::Instant::now() < deadline {
        let Ok(found) = alice
            .expect_control(Duration::from_millis(500), |m| match m {
                ServerMessage::NewMessage { .. } => Some(true),
                ServerMessage::MessageRejected { code: 429, .. } => Some(false),
                _ => None,
            })
            .await
        else {
            break;
        };
        if found { accepted += 1 } else { rejected += 1 }
    }
    assert_eq!((accepted, rejected), (5, 4));

    tokio::time::sleep(Duration::from_millis(2200)).await;
    alice
        .send(ClientMessage::SendMessage { channel_id: channel, content: "calm again".into(), attachments: vec![], client_msg_id: None, reply_to: None })
        .await?;
    alice.expect_control(Duration::from_secs(2), |m| matches!(m, ServerMessage::NewMessage { content, .. } if content == "calm again").then_some(())).await?;
    Ok(())
}
