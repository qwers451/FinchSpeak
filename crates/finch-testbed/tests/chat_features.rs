//! Replies, mentions, server-side read state, image thumbnails and history paging (stage 7, M2).

use std::time::Duration;

use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::{ChannelType, HistoryMessage, ReplyRef};
use finch_testbed::client::{TestClient, TEST_PASSWORD};
use finch_testbed::server::TestServer;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(3);

async fn say(client: &mut TestClient, channel_id: Uuid, text: &str, reply_to: Option<Uuid>) -> anyhow::Result<Uuid> {
    let wanted = text.to_string();
    client
        .send(ClientMessage::SendMessage {
            channel_id,
            content: text.to_string(),
            attachments: vec![],
            client_msg_id: None,
            reply_to,
        })
        .await?;
    client
        .expect_control(WAIT, move |m| match m {
            ServerMessage::NewMessage { message_id, content, .. } if *content == wanted => Some(*message_id),
            _ => None,
        })
        .await
}

fn history_entry(messages: &[HistoryMessage], id: Uuid) -> &HistoryMessage {
    messages.iter().find(|m| m.id == id).expect("message in history")
}

#[tokio::test]
async fn reply_carries_a_quote_live_and_in_history() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let bob = TestClient::register(&server, "bob").await?;
    let (channel, _) = alice.channel(ChannelType::Text);

    let original = say(&mut alice, channel, "Anyone up for a game tonight?", None).await?;
    bob.send(ClientMessage::SendMessage {
        channel_id: channel,
        content: "Count me in".into(),
        attachments: vec![],
        client_msg_id: None,
        reply_to: Some(original),
    })
    .await?;
    let live: Option<ReplyRef> = alice
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { content, reply_to, .. } if content == "Count me in" => Some(reply_to.clone()),
            _ => None,
        })
        .await?;
    let quote = live.expect("reply has a quote");
    assert_eq!(quote.message_id, original);
    assert_eq!(quote.author_name, "alice");
    assert_eq!(quote.snippet, "Anyone up for a game tonight?");
    assert!(!quote.deleted);

    let history = alice.api.fetch_messages(channel, None, 50).await?;
    let reply = history.messages.iter().find(|m| m.content == "Count me in").expect("reply in history");
    assert_eq!(reply.reply_to.as_ref().map(|r| r.message_id), Some(original));
    Ok(())
}

#[tokio::test]
async fn long_originals_are_cut_to_120_characters_in_the_quote() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    let long = "ж".repeat(300);
    let original = say(&mut alice, channel, &long, None).await?;
    say(&mut alice, channel, "short answer", Some(original)).await?;
    let history = alice.api.fetch_messages(channel, None, 50).await?;
    let reply = history.messages.iter().find(|m| m.content == "short answer").unwrap();
    assert_eq!(reply.reply_to.as_ref().unwrap().snippet.chars().count(), 120);
    Ok(())
}

#[tokio::test]
async fn deleting_the_original_marks_the_quote_as_deleted() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    let original = say(&mut alice, channel, "oops", None).await?;
    let reply = say(&mut alice, channel, "re: oops", Some(original)).await?;
    alice.api.delete_message(original).await?;

    let history = alice.api.fetch_messages(channel, None, 50).await?;
    let quote = history_entry(&history.messages, reply).reply_to.clone().expect("still a reply");
    assert!(quote.deleted);
    assert_eq!(quote.message_id, original);
    assert!(quote.snippet.is_empty() && quote.author_name.is_empty());
    Ok(())
}

#[tokio::test]
async fn reply_to_a_message_of_another_channel_is_rejected() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (general, _) = alice.channel(ChannelType::Text);
    let other = alice.api.create_channel("games", "text", None, None).await?;
    let in_other = say(&mut alice, Uuid::parse_str(&other.id)?, "elsewhere", None).await?;

    let client_msg_id = Uuid::new_v4();
    alice
        .send(ClientMessage::SendMessage {
            channel_id: general,
            content: "cross-channel".into(),
            attachments: vec![],
            client_msg_id: Some(client_msg_id),
            reply_to: Some(in_other),
        })
        .await?;
    alice
        .expect_control(WAIT, move |m| match m {
            ServerMessage::MessageRejected { client_msg_id: id, .. } if *id == client_msg_id => Some(()),
            _ => None,
        })
        .await?;
    Ok(())
}

#[tokio::test]
async fn mentions_are_resolved_by_login_and_ignore_lookalikes() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let bob = TestClient::register(&server, "bob").await?;
    let (channel, _) = alice.channel(ChannelType::Text);

    alice
        .send(ClientMessage::SendMessage {
            channel_id: channel,
            content: "hi @bob, and @Bob. also @nobody, mail me at alice@bob.example".into(),
            attachments: vec![],
            client_msg_id: None,
            reply_to: None,
        })
        .await?;
    let mentions = alice
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { mentions, .. } => Some(mentions.clone()),
            _ => None,
        })
        .await?;
    assert_eq!(mentions, vec![bob.session.user_id], "bob once; @nobody and the e-mail address do not count");

    let history = alice.api.fetch_messages(channel, None, 50).await?;
    assert_eq!(history.messages[0].mentions, vec![bob.session.user_id]);
    Ok(())
}

#[tokio::test]
async fn unread_counts_survive_across_devices_and_move_only_forward() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let bob = TestClient::register(&server, "bob").await?;
    let (channel, _) = alice.channel(ChannelType::Text);

    let first = say(&mut alice, channel, "one", None).await?;
    let second = say(&mut alice, channel, "two @bob", None).await?;
    let third = say(&mut alice, channel, "three", None).await?;

    let state = bob.api.server_state().await?;
    let entry = state.unread.iter().find(|u| u.channel_id == channel).expect("unread entry");
    assert_eq!((entry.count, entry.mentions, entry.first_unread_id), (3, 1, Some(first)));

    // Her own messages are never unread for her.
    assert!(alice.api.server_state().await?.unread.iter().all(|u| u.channel_id != channel));

    bob.api.mark_read(channel, first).await?;
    let entry = bob.api.server_state().await?.unread.into_iter().find(|u| u.channel_id == channel).unwrap();
    assert_eq!((entry.count, entry.mentions, entry.first_unread_id), (2, 1, Some(second)));

    // Marking something older does not bring messages back.
    bob.api.mark_read(channel, second).await?;
    bob.api.mark_read(channel, first).await?;
    let entry = bob.api.server_state().await?.unread.into_iter().find(|u| u.channel_id == channel).unwrap();
    assert_eq!((entry.count, entry.first_unread_id), (1, Some(third)));

    // A second device of the same account sees the same state.
    let mut second_device = server.api()?;
    let auth = second_device.login("bob", TEST_PASSWORD).await?;
    second_device.set_token(auth.token);
    let entry = second_device.server_state().await?.unread.into_iter().find(|u| u.channel_id == channel).unwrap();
    assert_eq!(entry.count, 1);

    bob.api.mark_read(channel, third).await?;
    assert!(bob.api.server_state().await?.unread.is_empty());
    Ok(())
}

#[tokio::test]
async fn marking_a_message_of_another_channel_is_refused() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let bob = TestClient::register(&server, "bob").await?;
    let (general, _) = alice.channel(ChannelType::Text);
    let other = alice.api.create_channel("games", "text", None, None).await?;
    let elsewhere = say(&mut alice, Uuid::parse_str(&other.id)?, "x", None).await?;
    assert!(bob.api.mark_read(general, elsewhere).await.is_err());
    assert!(bob.api.mark_read(Uuid::new_v4(), elsewhere).await.is_err());
    Ok(())
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[tokio::test]
async fn image_uploads_get_size_and_a_320px_thumbnail() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let info = alice.api.upload_file("wide.png", png(800, 400), "image/png").await?;
    assert_eq!((info.width, info.height, info.has_thumb), (Some(800), Some(400), true));

    let thumb = alice.api.download_thumb(info.id).await?;
    let decoded = image::load_from_memory(&thumb)?;
    assert_eq!(decoded.width(), 320);
    assert_eq!(decoded.height(), 160);
    Ok(())
}

#[tokio::test]
async fn small_images_are_not_enlarged() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let info = alice.api.upload_file("tiny.png", png(100, 50), "image/png").await?;
    let decoded = image::load_from_memory(&alice.api.download_thumb(info.id).await?)?;
    assert_eq!((decoded.width(), decoded.height()), (100, 50));
    Ok(())
}

#[tokio::test]
async fn files_that_are_not_images_have_no_thumbnail() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let text = alice.api.upload_file("notes.txt", b"hello".to_vec(), "text/plain").await?;
    assert!(!text.has_thumb);
    assert!(alice.api.download_thumb(text.id).await.is_err());

    // Claims to be a PNG but is not: stored as a plain file, no crash.
    let fake = alice.api.upload_file("fake.png", b"not really a png".to_vec(), "image/png").await?;
    assert!(!fake.has_thumb);
    assert_eq!(alice.api.download_file(fake.id).await?.as_ref(), b"not really a png");
    Ok(())
}

#[tokio::test]
async fn history_pages_walk_back_without_gaps_or_duplicates() -> anyhow::Result<()> {
    // 120 messages in a second would trip the flood limit.
    let server = TestServer::start_with(|c| c.auth.message_limit = 1000).await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    let mut sent = Vec::new();
    for i in 0..120 {
        sent.push(say(&mut alice, channel, &format!("message {i:03}"), None).await?);
    }

    let mut collected: Vec<Uuid> = Vec::new();
    let mut cursor: Option<Uuid> = None;
    loop {
        let page = alice.api.fetch_messages_page(channel, cursor, 50).await?;
        let mut ids: Vec<Uuid> = page.messages.iter().map(|m| m.id).collect();
        assert!(ids.len() <= 50);
        ids.extend(collected);
        collected = ids;
        match (page.has_more, page.messages.first()) {
            (true, Some(oldest)) => cursor = Some(oldest.id),
            _ => break,
        }
    }
    assert_eq!(collected, sent, "all 120 in order, once each");
    Ok(())
}
