//! Searching messages: Russian and English, prefixes, odd input, edits and deletions.

use std::time::Duration;

use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;
use uuid::Uuid;

async fn say(client: &mut TestClient, channel_id: Uuid, text: &str) -> anyhow::Result<Uuid> {
    let wanted = text.to_string();
    client
        .send(ClientMessage::SendMessage { channel_id, content: text.to_string(), attachments: vec![], client_msg_id: None, reply_to: None })
        .await?;
    client
        .expect_control(Duration::from_secs(3), move |m| match m {
            ServerMessage::NewMessage { message_id, content, .. } if *content == wanted => Some(*message_id),
            _ => None,
        })
        .await
}

#[tokio::test]
async fn finds_words_in_two_languages_ignoring_case_and_matching_the_start_of_the_last_word() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    say(&mut alice, channel, "Anyone up for a Rocket League match tonight?").await?;
    say(&mut alice, channel, "Сегодня вечером играем в Рокет Лигу").await?;
    say(&mut alice, channel, "Unrelated chatter").await?;

    let hits = alice.api.search_messages("rocket league", None, 30, 0).await?;
    assert_eq!(hits.len(), 1);
    assert!(hits[0].content.contains("Rocket League"));
    assert_eq!((hits[0].author_name.as_str(), hits[0].channel_name.as_str()), ("alice", "general"));

    assert_eq!(alice.api.search_messages("ROCK", None, 30, 0).await?.len(), 1, "prefix of the last word");
    assert_eq!(alice.api.search_messages("вечером игра", None, 30, 0).await?.len(), 1);
    assert_eq!(alice.api.search_messages("СЕГОДНЯ", None, 30, 0).await?.len(), 1);
    assert!(alice.api.search_messages("nothing like this", None, 30, 0).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn odd_input_never_fails() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    say(&mut alice, channel, "plain text").await?;
    for text in ["", "   ", "\"", "\"\"\"", "a OR", "NEAR(", "*", "-", ":", "(((", "text AND", "col:val", "' OR 1=1 --", "😀", &"x".repeat(5000)] {
        alice.api.search_messages(text, None, 30, 0).await.unwrap_or_else(|e| panic!("{text:?} failed: {e}"));
    }
    assert_eq!(alice.api.search_messages("plain \"", None, 30, 0).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn edits_and_deletions_are_followed_and_channels_can_be_chosen() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (general, _) = alice.channel(ChannelType::Text);
    let games = Uuid::parse_str(&alice.api.create_channel("games", "text", None, None).await?.id)?;
    let first = say(&mut alice, general, "banana split").await?;
    say(&mut alice, games, "banana bread").await?;

    assert_eq!(alice.api.search_messages("banana", None, 30, 0).await?.len(), 2);
    assert_eq!(alice.api.search_messages("banana", Some(games), 30, 0).await?.len(), 1);

    alice.api.edit_message(first, "cherry split").await?;
    assert_eq!(alice.api.search_messages("banana", None, 30, 0).await?.len(), 1);
    assert_eq!(alice.api.search_messages("cherry", None, 30, 0).await?.len(), 1);

    alice.api.delete_message(first).await?;
    assert!(alice.api.search_messages("cherry", None, 30, 0).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn results_come_newest_first_and_page() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| c.auth.message_limit = 1000).await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let (channel, _) = alice.channel(ChannelType::Text);
    for i in 0..5 {
        say(&mut alice, channel, &format!("needle number {i}")).await?;
    }
    let all = alice.api.search_messages("needle", None, 30, 0).await?;
    assert_eq!(all.len(), 5);
    let page = alice.api.search_messages("needle", None, 2, 2).await?;
    assert_eq!(page, all[2..4].to_vec());
    assert!(alice.api.search_messages("needle", None, 2, 5).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn search_needs_a_login() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    assert!(server.api()?.search_messages("anything", None, 10, 0).await.is_err());
    Ok(())
}
