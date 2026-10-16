//! Avatars (upload, re-encoding, access, ETag cache), presence status with text, last seen (stage 7, M4).

use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::UserStatus;
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(3);

fn http_status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

fn picture(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image).write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

#[tokio::test]
async fn an_uploaded_picture_becomes_a_128px_png_for_everyone() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    assert!(alice.api.me().await?.avatar_url.is_none());

    let me = alice.api.upload_avatar(picture(640, 360, image::ImageFormat::Jpeg), "image/jpeg").await?;
    let url = me.avatar_url.expect("avatar url after upload");
    assert!(url.starts_with(&format!("/api/users/{}/avatar", alice.session.user_id)), "got {url}");

    // Everyone is told, with the new address.
    let told = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::AvatarChanged { user_id, avatar_url } => Some((*user_id, avatar_url.clone())),
            _ => None,
        })
        .await?;
    assert_eq!(told, (alice.session.user_id, Some(url.clone())));

    // Any signed-in user can fetch it: 128x128, PNG, whatever was uploaded (centre crop, no stretching).
    let bytes = bob.api.download_avatar(alice.session.user_id).await?;
    assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G']);
    let decoded = image::load_from_memory(&bytes)?;
    assert_eq!((decoded.width(), decoded.height()), (128, 128));

    // The state a late joiner gets carries it too.
    let carol = TestClient::register(&server, "carol").await?;
    let state = carol.api.server_state().await?;
    let listed = state.online_users.iter().find(|user| user.id == alice.session.user_id).expect("alice online");
    assert_eq!(listed.avatar_url.as_deref(), Some(url.as_str()));
    Ok(())
}

#[tokio::test]
async fn png_and_webp_pictures_are_accepted_and_the_file_is_judged_by_its_bytes() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    alice.api.upload_avatar(picture(300, 300, image::ImageFormat::Png), "image/png").await?;
    // The announced type is not trusted: a PNG sent as text/plain still works, a text file sent as image/png does not.
    alice.api.upload_avatar(picture(200, 100, image::ImageFormat::Png), "text/plain").await?;
    let refused = alice.api.upload_avatar(b"this is not a picture".to_vec(), "image/png").await.expect_err("not an image");
    assert_eq!(http_status(&refused), Some(400));
    Ok(())
}

#[tokio::test]
async fn oversized_and_absurd_pictures_are_refused() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let too_big = vec![0u8; 3 * 1024 * 1024];
    assert_eq!(http_status(&alice.api.upload_avatar(too_big, "image/png").await.expect_err("3 MB")), Some(413));
    // A tiny file that claims to be huge (a decompression bomb) is refused before it is decoded.
    let mut bomb = picture(16, 16, image::ImageFormat::Png);
    bomb[16..20].copy_from_slice(&40_000u32.to_be_bytes());
    bomb[20..24].copy_from_slice(&40_000u32.to_be_bytes());
    assert_eq!(http_status(&alice.api.upload_avatar(bomb, "image/png").await.expect_err("bomb")), Some(400));
    assert!(alice.api.me().await?.avatar_url.is_none());
    Ok(())
}

#[tokio::test]
async fn removing_the_picture_tells_everyone_and_frees_the_address() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    alice.api.upload_avatar(picture(128, 128, image::ImageFormat::Png), "image/png").await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::AvatarChanged { avatar_url: Some(_), .. }).then_some(())).await?;
    alice.api.delete_avatar().await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::AvatarChanged { avatar_url: None, .. }).then_some(())).await?;
    assert!(alice.api.me().await?.avatar_url.is_none());
    assert_eq!(http_status(&bob.api.download_avatar(alice.session.user_id).await.expect_err("gone")), Some(404));
    // Removing twice is fine.
    alice.api.delete_avatar().await?;
    Ok(())
}

#[tokio::test]
async fn pictures_need_a_login_and_only_the_owner_changes_them() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    alice.api.upload_avatar(picture(128, 128, image::ImageFormat::Png), "image/png").await?;
    let anonymous = server.api()?;
    assert_eq!(http_status(&anonymous.download_avatar(alice.session.user_id).await.expect_err("no token")), Some(401));
    assert_eq!(http_status(&anonymous.upload_avatar(picture(16, 16, image::ImageFormat::Png), "image/png").await.expect_err("no token")), Some(401));
    Ok(())
}

#[tokio::test]
async fn an_unchanged_picture_answers_304_to_a_matching_etag() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    alice.api.upload_avatar(picture(128, 128, image::ImageFormat::Png), "image/png").await?;
    let (first, etag) = alice.api.download_avatar_cached(alice.session.user_id, None).await?;
    assert!(first.is_some());
    let etag = etag.expect("an ETag");
    let (second, _) = alice.api.download_avatar_cached(alice.session.user_id, Some(&etag)).await?;
    assert!(second.is_none(), "304: nothing to download again");
    Ok(())
}

#[tokio::test]
async fn status_and_text_are_saved_shown_to_others_and_kept_for_the_next_sign_in() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;

    alice.send(ClientMessage::SetStatus { status: UserStatus::DoNotDisturb, text: Some("  In a meeting  ".into()) }).await?;
    let (status, text) = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::PresenceUpdate { user_id, status, status_text } if *user_id == alice.session.user_id && *status == UserStatus::DoNotDisturb => {
                Some((*status, status_text.clone()))
            }
            _ => None,
        })
        .await?;
    assert_eq!((status, text.as_deref()), (UserStatus::DoNotDisturb, Some("In a meeting")));

    // A newcomer sees it in the state; so does alice after reconnecting.
    let carol = TestClient::register(&server, "carol").await?;
    let listed = carol.api.server_state().await?.online_users.into_iter().find(|u| u.id == alice.session.user_id).unwrap();
    assert_eq!((listed.status, listed.status_text.as_deref()), (UserStatus::DoNotDisturb, Some("In a meeting")));

    alice.reconnect(&server).await?;
    let again = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::PresenceUpdate { user_id, status, status_text } if *user_id == alice.session.user_id && *status != UserStatus::Offline => {
                Some((*status, status_text.clone()))
            }
            _ => None,
        })
        .await?;
    assert_eq!((again.0, again.1.as_deref()), (UserStatus::DoNotDisturb, Some("In a meeting")), "the choice survives a new session");
    Ok(())
}

#[tokio::test]
async fn offline_cannot_be_chosen_and_long_or_odd_text_is_cleaned() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    alice.send(ClientMessage::SetStatus { status: UserStatus::Offline, text: None }).await?;
    bob.expect_no_control(Duration::from_millis(400), |m| matches!(m, ServerMessage::PresenceUpdate { status: UserStatus::Offline, .. })).await?;

    let long = format!("{}\u{0}\n{}", "x".repeat(200), "y");
    alice.send(ClientMessage::SetStatus { status: UserStatus::Idle, text: Some(long) }).await?;
    let text = bob
        .expect_control(WAIT, |m| match m {
            ServerMessage::PresenceUpdate { status: UserStatus::Idle, status_text, .. } => Some(status_text.clone()),
            _ => None,
        })
        .await?
        .expect("some text");
    assert!(text.chars().count() <= 80, "cut to 80 characters, got {}", text.chars().count());
    assert!(!text.chars().any(char::is_control));
    Ok(())
}

#[tokio::test]
async fn an_empty_text_clears_it_and_going_offline_is_still_offline() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    alice.send(ClientMessage::SetStatus { status: UserStatus::Online, text: Some("Lunch".into()) }).await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::PresenceUpdate { status_text: Some(_), .. }).then_some(())).await?;
    alice.send(ClientMessage::SetStatus { status: UserStatus::Online, text: Some("   ".into()) }).await?;
    bob.expect_control(WAIT, |m| matches!(m, ServerMessage::PresenceUpdate { status: UserStatus::Online, status_text: None, .. }).then_some(())).await?;

    alice.session.connection.close(0u32.into(), b"bye");
    bob.expect_control(Duration::from_secs(20), |m| matches!(m, ServerMessage::PresenceUpdate { status: UserStatus::Offline, .. }).then_some(())).await?;
    Ok(())
}

#[tokio::test]
async fn the_admin_list_shows_when_someone_was_last_seen() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let admin = TestClient::register(&server, "boss").await?;
    let carol = TestClient::register(&server, "carol").await?;
    let before = admin.api.admin_users().await?;
    assert!(before.iter().find(|u| u.username == "carol").unwrap().last_seen.is_none(), "never left yet");
    carol.session.connection.close(0u32.into(), b"bye");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let after = admin.api.admin_users().await?;
    assert!(after.iter().find(|u| u.username == "carol").unwrap().last_seen.is_some());
    Ok(())
}
