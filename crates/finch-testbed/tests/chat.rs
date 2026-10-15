//! Chat scenarios: real-time broadcast, persisted history, file attachments.

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;
use rand::{Rng, SeedableRng};

const WAIT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn message_broadcast_to_all() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let mut c = TestClient::register(&server, "carol").await?;
    let (text_id, _) = a.channel(ChannelType::Text);

    a.send(ClientMessage::SendMessage {
        channel_id: text_id,
        content: "hello all".to_string(),
        attachments: vec![],
    })
    .await?;

    let a_id = a.session.user_id;
    for client in [&mut b, &mut c] {
        let author = client
            .expect_control(WAIT, |m| match m {
                ServerMessage::NewMessage { content, author_id, .. } if content == "hello all" => {
                    Some(*author_id)
                }
                _ => None,
            })
            .await?;
        assert_eq!(author, a_id);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "AUD-011"]
async fn history_persisted_in_order() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let (text_id, _) = a.channel(ChannelType::Text);

    let sent = ["first", "second", "third"];
    for content in sent {
        a.send(ClientMessage::SendMessage {
            channel_id: text_id,
            content: content.to_string(),
            attachments: vec![],
        })
        .await?;
        a.expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { content: c, .. } if c == content => Some(()),
            _ => None,
        })
        .await?;
    }

    let history = a.api.fetch_messages(text_id, None, 50).await?;
    let contents: Vec<&str> = history.messages.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(contents, sent);
    Ok(())
}

#[tokio::test]
async fn file_upload_download_roundtrip() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let a = TestClient::register(&server, "alice").await?;

    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    let data: Vec<u8> = (0..100 * 1024).map(|_| rng.r#gen()).collect();

    let info = a.api.upload_file("data.bin", data.clone(), "application/octet-stream").await?;
    let downloaded = a.api.download_file(info.id).await?;
    assert_eq!(downloaded.as_ref(), data.as_slice());
    Ok(())
}

#[tokio::test]
async fn file_over_limit_rejected() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let a = TestClient::register(&server, "alice").await?;

    let data = vec![7u8; 1536 * 1024];
    match a.api.upload_file("big.bin", data, "application/octet-stream").await {
        Err(ClientError::Http { status, .. }) => {
            assert!(status == 413 || status == 400, "unexpected status {status}");
        }
        other => panic!("expected 413/400, got {other:?}"),
    }
    Ok(())
}
