//! Channel lifecycle: a channel created over REST is announced and usable right away (AUD-009).

use std::time::Duration;

use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn created_voice_channel_is_announced_and_joinable() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let admin = TestClient::register(&server, "admin").await?; // first user is admin
    let mut bob = TestClient::register(&server, "bob").await?;

    let created = admin.api.create_channel("raid", "voice", None, None).await?;
    let channel_id: uuid::Uuid = created.id.parse()?;

    let announced = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::ChannelCreated { channel } if channel.id == channel_id => Some(channel.clone()),
            _ => None,
        })
        .await?;
    assert_eq!(announced.channel_type, ChannelType::Voice);
    assert_eq!(announced.short_id, created.short_id);

    bob.send(ClientMessage::JoinVoice { channel_id }).await?;
    let me = bob.session.session_peer_id;
    bob.expect_control(WAIT, |m| match m {
        ServerMessage::UserJoinedVoice { peer_id, channel_id: c, .. } if *peer_id == me && *c == channel_id => Some(()),
        _ => None,
    })
    .await?;
    Ok(())
}
