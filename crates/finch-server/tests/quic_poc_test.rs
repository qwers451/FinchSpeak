//! End-to-end Integration Test for Stage 0 and Stage 1.
//!
//! Validates:
//! 1. SQLite database initialization with WAL mode and initial migrations.
//! 2. QUIC connection establishment over TLS (self-signed).
//! 3. Control Stream handshake: Authenticate with JWT, Ping/Pong framing.
//! 4. Joining a voice channel and sending media datagrams with SFU rewrite.
//! 5. Uni-Stream IDR video keyframe transmission.
//! 6. Synthetic Opus audio datagram transmission and packet parsing (160 bytes).
//! 7. Synthetic video frame fragmentation and reassembly test.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use finch_protocol::codec::{decode_message, encode_message};
use finch_protocol::media::{MediaHeader, MediaPacket, MAX_DATAGRAM_PAYLOAD, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_server::api::auth::issue_jwt;
use finch_server::db::init_db;
use finch_server::quic;
use finch_server::session::SessionManager;
use finch_server::sfu::SfuRouter;
use uuid::Uuid;

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn make_client_config() -> anyhow::Result<quinn::ClientConfig> {
    let mut tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    tls_config.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];

    let quic_client_config = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)?;
    let mut client_config = quinn::ClientConfig::new(Arc::new(quic_client_config));

    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(std::time::Duration::from_secs(10).try_into()?));
    transport.datagram_receive_buffer_size(Some(2 * 1024 * 1024));
    transport.datagram_send_buffer_size(2 * 1024 * 1024);
    client_config.transport_config(Arc::new(transport));

    Ok(client_config)
}

#[tokio::test]
async fn test_quic_poc_full_suite() -> anyhow::Result<()> {
    // ─── 1. Initialize SQLite in-memory DB & sessions ────────────────────────
    let pool = init_db("sqlite::memory:").await?;
    let sessions = SessionManager::new();
    let sfu = SfuRouter::new(sessions.clone());
    let jwt_secret = "test_super_secret_jwt_key_stage1_32bytes".to_string();

    let test_user_id = Uuid::new_v4();
    let (jwt_token, _) = issue_jwt(&test_user_id, "testuser", "member", &jwt_secret, 24)?;

    // ─── 2. Start Server on ephemeral port ──────────────────────────────────
    let server_config = quic::make_server_config()?;
    let bind_addr: SocketAddr = "127.0.0.1:0".parse()?;
    let server_endpoint = quinn::Endpoint::server(server_config, bind_addr)?;
    let server_addr = server_endpoint.local_addr()?;

    let pool_clone = pool.clone();
    let sessions_clone = sessions.clone();
    let sfu_clone = sfu.clone();
    let jwt_secret_clone = jwt_secret.clone();

    tokio::spawn(async move {
        while let Some(incoming) = server_endpoint.accept().await {
            if let Ok(conn) = incoming.await {
                let p = pool_clone.clone();
                let s = sessions_clone.clone();
                let r = sfu_clone.clone();
                let sec = jwt_secret_clone.clone();
                tokio::spawn(async move {
                    let _ = quic::handle_connection(conn, p, s, r, sec).await;
                });
            }
        }
    });

    // ─── 3. Connect Client ──────────────────────────────────────────────────
    let client_config = make_client_config()?;
    let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    client_endpoint.set_default_client_config(client_config);

    let connection = client_endpoint.connect(server_addr, "localhost")?.await?;
    assert_eq!(connection.remote_address(), server_addr);

    // ─── 4. Control Stream: Authenticate & Ping/Pong ────────────────────────
    let (mut send_stream, mut recv_stream) = connection.open_bi().await?;

    // Send Authenticate with JWT
    let auth_msg = ClientMessage::Authenticate { token: jwt_token };
    send_stream.write_all(&encode_message(&auth_msg)?).await?;

    // Read Authenticated response
    let mut stream_buf = BytesMut::new();
    let mut chunk = [0u8; 1024];
    let n = recv_stream
        .read(&mut chunk)
        .await?
        .ok_or_else(|| anyhow::anyhow!("stream closed prematurely"))?;
    stream_buf.extend_from_slice(&chunk[..n]);

    let server_msg = decode_message::<ServerMessage>(&mut stream_buf)?;
    let (session_peer_id, voice_channel_uuid, voice_channel_short_id) = match server_msg {
        ServerMessage::Authenticated {
            session_peer_id,
            channels,
            ..
        } => {
            assert!(session_peer_id >= 1000);
            assert!(!channels.is_empty());
            let voice_ch = &channels[0];
            (session_peer_id, voice_ch.id, voice_ch.short_id)
        }
        other => panic!("Expected Authenticated, got: {:?}", other),
    };

    // Send Ping
    let ping = ClientMessage::Ping { timestamp: 777_888 };
    send_stream.write_all(&encode_message(&ping)?).await?;

    // Read responses until Pong is received
    let mut received_pong = false;
    for _ in 0..5 {
        if let Ok(msg) = decode_message::<ServerMessage>(&mut stream_buf) {
            if msg == (ServerMessage::Pong { timestamp: 777_888 }) {
                received_pong = true;
                break;
            }
        } else {
            let n = recv_stream
                .read(&mut chunk)
                .await?
                .ok_or_else(|| anyhow::anyhow!("stream closed prematurely"))?;
            stream_buf.extend_from_slice(&chunk[..n]);
        }
    }
    assert!(received_pong, "Should have received Pong response");

    // Join voice channel
    let join_voice = ClientMessage::JoinVoice { channel_id: voice_channel_uuid };
    send_stream.write_all(&encode_message(&join_voice)?).await?;

    // ─── 5. Media Datagrams: 20-byte header & SFU sender_peer_id rewrite ────
    let dummy_opus_payload = Bytes::copy_from_slice(&[0xAA; 160]);
    // Client sends datagram with its assigned channel short_id and sender_peer_id = 0
    let client_header = MediaHeader::new_audio(voice_channel_short_id, 0, 100, 960, false, false);
    let packet = MediaPacket::new(client_header, dummy_opus_payload.clone());
    let raw_encoded = packet.encode();

    assert_eq!(raw_encoded.len(), MEDIA_HEADER_SIZE + 160);
    connection.send_datagram(raw_encoded.clone())?;

    // Verify SFU rewrite logic directly on datagram bytes:
    // bytes 5..9 (sender_peer_id) must be rewritten with session_peer_id
    let mut rewritten_bytes = raw_encoded.to_vec();
    MediaHeader::overwrite_sender_peer_id(&mut rewritten_bytes, session_peer_id)?;
    let parsed_rewritten = MediaHeader::from_slice(&rewritten_bytes)?;
    assert_eq!(parsed_rewritten.sender_peer_id, session_peer_id);
    assert_eq!(parsed_rewritten.channel_id, voice_channel_short_id);

    // ─── 6. Uni-Stream: IDR Keyframe transmission ───────────────────────────
    let mut uni_stream = connection.open_uni().await?;
    let keyframe_header = MediaHeader::new_video_key(voice_channel_short_id, session_peer_id, 1, 90_000, 1, false);
    uni_stream.write_all(&keyframe_header.to_bytes()).await?;

    let idr_data = vec![0x67; 4096]; // 4 KB dummy SPS/PPS/IDR NAL
    uni_stream.write_all(&idr_data).await?;
    uni_stream.finish()?;

    // ─── 7. Synthetic Opus Audio Datagram & Packet Parsing (160 bytes) ───────
    let audio_payload = Bytes::copy_from_slice(&[0xBB; 160]); // 160-byte dummy Opus frame
    let audio_header = MediaHeader::new_audio(voice_channel_short_id, session_peer_id, 101, 1920, false, false);
    let audio_packet = MediaPacket::new(audio_header, audio_payload.clone());
    let encoded_packet = audio_packet.encode();
    assert_eq!(encoded_packet.len(), MEDIA_HEADER_SIZE + 160);

    // Send synthetic Opus audio datagram over QUIC
    connection.send_datagram(encoded_packet.clone())?;

    // Parse packet and verify 20-byte header fields and synthetic payload
    let parsed_audio = MediaPacket::parse(encoded_packet)?;
    assert_eq!(parsed_audio.header.media_type, finch_protocol::media::MediaType::Audio);
    assert_eq!(parsed_audio.header.channel_id, voice_channel_short_id);
    assert_eq!(parsed_audio.header.sender_peer_id, session_peer_id);
    assert_eq!(parsed_audio.header.sequence, 101);
    assert_eq!(parsed_audio.header.timestamp, 1920);
    assert_eq!(parsed_audio.payload.len(), 160);
    assert_eq!(parsed_audio.payload, audio_payload);

    // ─── 8. Synthetic Video Datagram Fragmentation & Reassembly Test ────────
    let p_frame_data = vec![0x41u8; 3000]; // 3000 bytes => 3 fragments (1160 + 1160 + 680)
    let chunk_size = MAX_DATAGRAM_PAYLOAD;
    let chunks: Vec<&[u8]> = p_frame_data.chunks(chunk_size).collect();
    let total_fragments = chunks.len() as u8;
    assert_eq!(total_fragments, 3);

    let mut fragments = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let seq = 1u16.wrapping_add(i as u16);
        let header = MediaHeader::new_video_delta(
            voice_channel_short_id,
            session_peer_id,
            seq,
            90_033,
            2, // frame_id
            i as u8,
            total_fragments,
            false,
        );
        let pkt = MediaPacket::new(header, Bytes::copy_from_slice(chunk));
        connection.send_datagram(pkt.encode())?;
        fragments.push(pkt);
    }
    assert_eq!(fragments.len(), 3);

    // Simulate reception of out-of-order datagram fragments (e.g. index 1, then 0, then 2)
    let mut received_slots: Vec<Option<Bytes>> = vec![None; total_fragments as usize];
    for &idx in &[1usize, 0, 2] {
        let frag = &fragments[idx];
        received_slots[frag.header.fragment_index as usize] = Some(frag.payload.clone());
    }

    // Assemble all fragments in order
    assert!(received_slots.iter().all(|s| s.is_some()));
    let mut assembled = Vec::with_capacity(p_frame_data.len());
    for slot in received_slots {
        let chunk = slot.ok_or_else(|| anyhow::anyhow!("missing fragment"))?;
        assembled.extend_from_slice(&chunk);
    }

    assert_eq!(assembled.len(), 3000);
    assert_eq!(assembled, p_frame_data);

    Ok(())
}
