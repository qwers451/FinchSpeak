//! File attachments: size limit, non-ASCII names, access control, linking to messages.

use std::time::Duration;

use finch_client_core::api::ApiClient;
use finch_client_core::error::ClientError;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

/// The configured limit is the real limit, not axum's default 2 MB body limit (AUD-012).
#[tokio::test]
async fn upload_up_to_configured_limit_succeeds() -> anyhow::Result<()> {
    let server = TestServer::start_with(|c| c.storage.max_file_size_mb = 5).await?;
    let a = TestClient::register(&server, "alice").await?;
    let data = vec![42u8; 3 * 1024 * 1024];
    let info = a.api.upload_file("big.bin", data.clone(), "application/octet-stream").await?;
    assert_eq!(a.api.download_file(info.id).await?.len(), data.len());
    Ok(())
}

/// Files with Cyrillic names download fine (AUD-013 did not reproduce; kept as a regression guard).
#[tokio::test]
async fn non_ascii_filename_downloads() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let a = TestClient::register(&server, "alice").await?;
    let info = a.api.upload_file("отчёт за май.pdf", b"%PDF-1.4 test".to_vec(), "application/pdf").await?;
    assert_eq!(info.filename, "отчёт за май.pdf");
    assert_eq!(a.api.download_file(info.id).await?.as_ref(), b"%PDF-1.4 test");
    Ok(())
}

/// Downloading requires a valid token (AUD-014).
#[tokio::test]
#[ignore = "AUD-014"]
async fn download_requires_auth() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let a = TestClient::register(&server, "alice").await?;
    let info = a.api.upload_file("secret.txt", b"secret".to_vec(), "text/plain").await?;

    let anonymous = ApiClient::new(&server.http_url)?;
    match anonymous.download_file(info.id).await {
        Err(ClientError::Http { status, .. }) => assert_eq!(status, 401),
        other => panic!("expected 401 without a token, got {other:?}"),
    }
    Ok(())
}

/// Attachments arrive with the real-time message, and nobody can take over someone
/// else's attachment (AUD-015).
#[tokio::test]
#[ignore = "AUD-015"]
async fn attachments_are_delivered_and_cannot_be_stolen() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let (text_id, _) = a.channel(ChannelType::Text);

    let file = a.api.upload_file("photo.png", vec![1u8; 512], "image/png").await?;
    a.send(ClientMessage::SendMessage {
        channel_id: text_id,
        content: "look".to_string(),
        attachments: vec![file.id],
    })
    .await?;
    let delivered = b
        .expect_control(WAIT, |m| match m {
            ServerMessage::NewMessage { content, attachments, .. } if content == "look" => Some(attachments.clone()),
            _ => None,
        })
        .await?;
    assert_eq!(delivered.iter().map(|x| x.id).collect::<Vec<_>>(), vec![file.id]);

    // Bob references Alice's attachment in his own message.
    b.send(ClientMessage::SendMessage {
        channel_id: text_id,
        content: "mine now".to_string(),
        attachments: vec![file.id],
    })
    .await?;
    a.expect_control(WAIT, |m| match m {
        ServerMessage::NewMessage { content, .. } if content == "mine now" => Some(()),
        _ => None,
    })
    .await?;

    let history = a.api.fetch_messages(text_id, None, 50).await?;
    let attachments_of = |content: &str| -> Vec<uuid::Uuid> {
        history
            .messages
            .iter()
            .find(|m| m.content == content)
            .map(|m| m.attachments.iter().map(|x| x.id).collect())
            .unwrap_or_default()
    };
    assert_eq!(attachments_of("look"), vec![file.id]);
    assert!(attachments_of("mine now").is_empty());
    Ok(())
}
