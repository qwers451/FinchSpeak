//! Protocol level 1.7 and the feature list a 1.7 server announces.
use finch_protocol::messages::ServerMessage;
use finch_protocol::CLIENT_LEVEL_1_7;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;
use uuid::Uuid;

#[test]
fn flock_calls_need_client_level_1_7() {
    assert_eq!(CLIENT_LEVEL_1_7, 170);
    let call = ServerMessage::FlockCall { from_user_id: Uuid::nil(), from_name: "a".into(), channel_id: Uuid::nil() };
    assert_eq!(call.minimum_client_level(), CLIENT_LEVEL_1_7);
}

#[tokio::test]
async fn a_1_7_server_announces_the_four_new_features() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let features = alice.api.server_state().await?.features;
    for feature in ["soundboard", "voice-messages", "flock", "health"] {
        assert!(features.contains(&feature.to_string()), "missing feature {feature}");
    }
    Ok(())
}

#[tokio::test]
async fn a_disabled_soundboard_is_not_announced() -> anyhow::Result<()> {
    let server = TestServer::start_with(|config| config.soundboard.enabled = false).await?;
    let alice = TestClient::register(&server, "alice").await?;
    let features = alice.api.server_state().await?.features;
    assert!(!features.contains(&"soundboard".to_string()));
    assert!(features.contains(&"flock".to_string()));
    Ok(())
}
