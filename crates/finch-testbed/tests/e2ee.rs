//! E2EE at protocol level: an encrypted payload survives the SFU unchanged.

use std::time::Duration;

use finch_protocol::e2ee::{decrypt_media_payload, encrypt_media_payload};
use finch_protocol::media::{MediaHeader, MediaPacket, MEDIA_HEADER_SIZE};
use finch_testbed::client::TestClient;
use finch_testbed::server::TestServer;

#[tokio::test]
async fn encrypted_audio_survives_sfu() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let mut a = TestClient::register(&server, "alice").await?;
    let mut b = TestClient::register(&server, "bob").await?;
    let mut c = TestClient::register(&server, "carol").await?;
    let (_, voice_short) = a.join_voice().await?;
    b.join_voice().await?;
    c.join_voice().await?;
    b.drain_datagrams();
    c.drain_datagrams();

    let key = [7u8; 32];
    let plaintext = b"opus-payload-for-e2ee-check".to_vec();
    let mut header = MediaHeader::new_audio(voice_short, a.session.session_peer_id, 5, 960, false, false);
    let ciphertext = encrypt_media_payload(&mut header, &plaintext, &key)?;
    a.session.send_datagram(MediaPacket::new(header, ciphertext).encode())?;

    let at_b = b.wait_datagram(Duration::from_secs(2)).await?;
    let hdr_b = MediaHeader::from_slice(&at_b[..MEDIA_HEADER_SIZE])?;
    let decrypted = decrypt_media_payload(&hdr_b, &at_b[MEDIA_HEADER_SIZE..], &key)?;
    assert_eq!(decrypted.as_ref(), plaintext.as_slice());

    let at_c = c.wait_datagram(Duration::from_secs(2)).await?;
    let hdr_c = MediaHeader::from_slice(&at_c[..MEDIA_HEADER_SIZE])?;
    assert!(decrypt_media_payload(&hdr_c, &at_c[MEDIA_HEADER_SIZE..], &[8u8; 32]).is_err());
    Ok(())
}
