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
async fn oversized_login_names_are_rejected_without_creating_lockout_entries() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let api = server.api()?;
    let username = "x".repeat(16_384);
    for _ in 0..6 {
        assert_eq!(status(&api.login(&username, "wrong-password").await.expect_err("oversized login")), Some(401));
    }
    Ok(())
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

#[tokio::test]
async fn a_flood_of_status_changes_is_cut_off() -> anyhow::Result<()> {
    use finch_protocol::models::UserStatus;
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    for i in 0..40 {
        alice.send(ClientMessage::SetStatus { status: if i % 2 == 0 { UserStatus::Idle } else { UserStatus::Online }, text: None }).await?;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let mut seen = 0;
    while bob
        .expect_control(Duration::from_millis(300), |m| matches!(m, ServerMessage::PresenceUpdate { user_id, .. } if *user_id == alice.session.user_id).then_some(()))
        .await
        .is_ok()
    {
        seen += 1;
    }
    assert!((1..=10).contains(&seen), "got {seen} updates for 40 requests");
    Ok(())
}

#[tokio::test]
async fn picture_changes_are_limited_per_hour() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let image = image::RgbImage::from_fn(32, 32, |x, y| image::Rgb([x as u8, y as u8, 1]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image).write_to(&mut bytes, image::ImageFormat::Png)?;
    let png = bytes.into_inner();
    for _ in 0..10 {
        alice.api.upload_avatar(png.clone(), "image/png").await?;
    }
    assert_eq!(status(&alice.api.upload_avatar(png, "image/png").await.expect_err("eleventh")), Some(429));
    Ok(())
}

#[tokio::test]
async fn reaction_flooding_is_slowed_down() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    alice
        .send(ClientMessage::SendMessage { channel_id: channel, content: "react to me".into(), attachments: vec![], client_msg_id: None, reply_to: None })
        .await?;
    let message = alice
        .expect_control(Duration::from_secs(2), |m| match m {
            ServerMessage::NewMessage { message_id, .. } => Some(*message_id),
            _ => None,
        })
        .await?;
    let mut refused = 0;
    for i in 0..40 {
        let emoji = char::from_u32(0x1F600 + (i % 15)).map(String::from).unwrap_or_default();
        if let Err(error) = alice.api.add_reaction(message, &emoji).await {
            assert_eq!(status(&error), Some(429));
            refused += 1;
        }
    }
    assert!(refused >= 5, "only {refused} refused of 40");
    Ok(())
}
