//! Comprehensive Load Test for FinchSpeak QUIC SFU Server.
//!
//! Validates:
//! 1. Concurrent QUIC connections from 10 clients on ephemeral UDP ports.
//! 2. Control stream authentication via JWT (Stream 0) and voice channel joining.
//! 3. Screen share lifecycle: streamer starts screen share, 9 viewers subscribe.
//! 4. Screen share video transmission:
//!    - 32 KB IDR keyframe over QUIC Uni-Stream (MediaType::VideoKey = 0x02).
//!    - Fragmented delta frame (3480 bytes in 1160-byte fragments, MediaType::VideoDelta = 0x03).
//!    - Verified reassembly on all 9 subscribers.
//! 5. Voice load simulation:
//!    - 10 concurrent clients transmitting synthetic Opus datagrams at 50 pkts/s (20 ms interval).
//!    - 100 packets per client = 1,000 total datagrams sent over ~2.0 seconds.
//!    - SFU fan-out delivery of ~9,000 datagrams to room participants.
//! 6. SFU security validation:
//!    - SFU unconditionally rewrites bytes 5..9 (sender_peer_id) to the authenticated session peer ID.
//! 7. Zero packet corruption, zero unwrap/expect in logic, error propagation via Result/thiserror.
//! 8. Real-time latency and throughput metrics calculation (min, avg, p50, p95, p99, max).

use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use finch_protocol::codec::{decode_message, encode_message};
use finch_protocol::media::{
    MediaHeader, MediaPacket, MediaType, MAX_DATAGRAM_PAYLOAD, MEDIA_HEADER_SIZE,
};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::VideoCodec;
use finch_server::api::auth::issue_jwt;
use finch_server::db::init_db;
use finch_server::quic;
use finch_server::session::SessionManager;
use finch_server::sfu::SfuRouter;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Custom error types for load testing validation.
#[derive(Debug, thiserror::Error)]
pub enum LoadTestError {
    #[error("Timeout waiting for {0}")]
    Timeout(String),

    #[error("Unexpected message on control stream: {0:?}")]
    UnexpectedMessage(String),

    #[error("SFU security check failed: expected sender_peer_id {expected}, got {actual}")]
    SfuSenderRewriteFailed { expected: u32, actual: u32 },

    #[error("Packet integrity check failed: {0}")]
    PacketIntegrityFailed(String),

    #[error("Voice delivery rate too low: received {received} of {expected} ({rate:.2}%)")]
    DeliveryRateTooLow {
        received: u64,
        expected: u64,
        rate: f64,
    },
}

/// TLS certificate verifier that accepts self-signed certificates for local testing.
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

/// Construct QUIC client configuration with high datagram buffer limits.
fn make_client_config() -> anyhow::Result<quinn::ClientConfig> {
    let mut tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    tls_config.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];

    let quic_client_config = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)?;
    let mut client_config = quinn::ClientConfig::new(Arc::new(quic_client_config));

    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into()?));
    transport.max_concurrent_uni_streams(256u32.into());
    transport.datagram_receive_buffer_size(Some(4 * 1024 * 1024));
    transport.datagram_send_buffer_size(4 * 1024 * 1024);
    client_config.transport_config(Arc::new(transport));

    Ok(client_config)
}

/// Simulated client instance.
struct ConnectedClient {
    user_id: Uuid,
    _username: String,
    session_peer_id: u32,
    connection: quinn::Connection,
    control_send: quinn::SendStream,
}

/// Per-client receiver statistics for media datagrams.
#[derive(Default)]
struct ClientDatagramStats {
    /// Audio datagrams received.
    audio_received: u64,
    /// Round-trip delivery latency in microseconds for audio packets.
    audio_latencies_us: Vec<u64>,
    /// Distinct sender peer IDs seen for audio.
    audio_senders: HashSet<u32>,

    /// Video delta packets received.
    video_delta_received: u64,
    /// Video delta fragments received for reassembly (fragment_index -> bytes).
    video_delta_fragments: BTreeMap<u8, Bytes>,

    /// Count of corrupted / unparseable packets.
    corrupted_count: u64,
    /// Count of packets with wrong or unverified sender peer IDs.
    unauthorized_senders: u64,
}

#[tokio::test]
async fn test_full_sfu_load_and_screen_share_suite() -> anyhow::Result<()> {
    let test_suite_start = Instant::now();

    // ─── 1. Initialize SQLite Database & SFU Core ────────────────────────────
    let pool = init_db("sqlite::memory:").await?;
    let sessions = SessionManager::new();
    quic::sync_channels_from_db(&pool, &sessions).await?;
    let sfu = SfuRouter::new(sessions.clone());
    let jwt_secret = "finchspeak_load_test_jwt_secret_key_32_bytes!!".to_string();

    // Query seeded default voice channel
    let (c_id_str, short_id_val): (String, i64) = sqlx::query_as::<_, (String, i64)>(
        "SELECT id, short_id FROM channels WHERE channel_type = 'voice' LIMIT 1",
    )
    .fetch_one(&pool)
    .await?;
    let voice_channel_uuid = Uuid::parse_str(&c_id_str)?;
    let voice_channel_short_id = short_id_val as u32;

    // ─── 2. Start QUIC Server Endpoint on Ephemeral Port ────────────────────
    let server_config = quic::make_server_config()?;
    let bind_addr: SocketAddr = "127.0.0.1:0".parse()?;
    let server_endpoint = quinn::Endpoint::server(server_config, bind_addr)?;
    let server_addr = server_endpoint.local_addr()?;

    let pool_clone = pool.clone();
    let sessions_clone = sessions.clone();
    let sfu_clone = sfu.clone();
    let jwt_secret_clone = jwt_secret.clone();

    let _server_task = tokio::spawn(async move {
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

    // ─── 3. Connect 10 QUIC Clients Concurrently ────────────────────────────
    const CLIENT_COUNT: usize = 10;
    let mut clients: Vec<ConnectedClient> = Vec::with_capacity(CLIENT_COUNT);

    for i in 0..CLIENT_COUNT {
        let user_id = Uuid::new_v4();
        let username = format!("user_{i:02}");
        sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES (?, ?, ?, '', 0)")
            .bind(user_id.to_string()).bind(&username).bind(&username).execute(&pool).await?;
        let (jwt_token, jti) = issue_jwt(&user_id, &username, "member", &jwt_secret, 24)?;
        sqlx::query("INSERT INTO sessions (id, user_id, expires_at, created_at) VALUES (?, ?, strftime('%s', 'now') + 86400, 0)")
            .bind(jti).bind(user_id.to_string()).execute(&pool).await?;

        let client_config = make_client_config()?;
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
        client_endpoint.set_default_client_config(client_config);

        let connection = client_endpoint.connect(server_addr, "localhost")?.await?;

        // Open bidirectional Control Stream (Stream 0)
        let (mut control_send, mut control_recv) = connection.open_bi().await?;

        // Authenticate with JWT
        let auth_msg = ClientMessage::Authenticate { token: jwt_token };
        control_send.write_all(&encode_message(&auth_msg)?).await?;

        // Read Authenticated response
        let mut stream_buf = BytesMut::new();
        let mut chunk = [0u8; 2048];
        let session_peer_id = loop {
            let n = control_recv
                .read(&mut chunk)
                .await?
                .ok_or_else(|| LoadTestError::Timeout(format!("Stream closed before auth for {username}")))?;
            stream_buf.extend_from_slice(&chunk[..n]);

            match decode_message::<ServerMessage>(&mut stream_buf) {
                Ok(ServerMessage::Authenticated {
                    session_peer_id, ..
                }) => break session_peer_id,
                Ok(other) => {
                    return Err(LoadTestError::UnexpectedMessage(format!("{other:?}")).into());
                }
                Err(finch_protocol::codec::CodecError::Incomplete { .. }) => continue,
                Err(e) => return Err(e.into()),
            }
        };

        // Drain incoming control messages (e.g. presence, room events) in background
        tokio::spawn(async move {
            let mut drain_buf = [0u8; 4096];
            while let Ok(Some(_)) = control_recv.read(&mut drain_buf).await {}
        });

        // Join Voice Channel
        let join_msg = ClientMessage::JoinVoice {
            channel_id: voice_channel_uuid,
        };
        control_send.write_all(&encode_message(&join_msg)?).await?;

        clients.push(ConnectedClient {
            user_id,
            _username: username,
            session_peer_id,
            connection,
            control_send,
        });
    }

    // Await until all 10 clients are registered in the server's voice room
    let join_wait_start = Instant::now();
    loop {
        let count = match sessions.voice_rooms.get(&voice_channel_short_id) {
            Some(r) => r.participants.len(),
            None => 0,
        };
        if count == CLIENT_COUNT {
            break;
        }
        if join_wait_start.elapsed() > Duration::from_secs(5) {
            return Err(LoadTestError::Timeout(format!(
                "Voice room participants count={count}, expected {CLIENT_COUNT}"
            ))
            .into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // ─── 4. Screen Share Lifecycle: Streamer & 9 Subscribers ────────────────
    let streamer_user_id = clients[0].user_id;
    let streamer_peer_id = clients[0].session_peer_id;

    // Client 0 starts screen share
    let start_screen_msg = ClientMessage::StartScreenShare {
        codec: VideoCodec::H264,
        width: 1920,
        height: 1080,
        fps: 60,
    };
    clients[0]
        .control_send
        .write_all(&encode_message(&start_screen_msg)?)
        .await?;

    // Clients 1..10 subscribe to Client 0's screen share
    for viewer in &mut clients[1..CLIENT_COUNT] {
        let sub_msg = ClientMessage::SubscribeScreenShare {
            target_user_id: streamer_user_id,
        };
        viewer
            .control_send
            .write_all(&encode_message(&sub_msg)?)
            .await?;
    }

    // Await until all 9 viewers are registered in streamer's screen_subscribers
    let sub_wait_start = Instant::now();
    loop {
        let count = match sessions.voice_rooms.get(&voice_channel_short_id) {
            Some(r) => match r.screen_subscribers.get(&streamer_peer_id) {
                Some(viewers) => viewers.len(),
                None => 0,
            },
            None => 0,
        };
        if count == CLIENT_COUNT - 1 {
            break;
        }
        if sub_wait_start.elapsed() > Duration::from_secs(5) {
            return Err(LoadTestError::Timeout(format!(
                "Screen subscribers count={count}, expected {}",
                CLIENT_COUNT - 1
            ))
            .into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // ─── 5. Screen Share IDR Keyframe via Uni-Stream (32 KB) ────────────────
    // Prepare 9 subscriber Uni-Stream receivers before streamer transmits
    let mut keyframe_receivers = Vec::new();
    for viewer in &clients[1..CLIENT_COUNT] {
        let conn = viewer.connection.clone();
        let handle = tokio::spawn(async move {
            let mut recv = conn.accept_uni().await?;
            let mut hdr_bytes = [0u8; MEDIA_HEADER_SIZE];
            recv.read_exact(&mut hdr_bytes).await?;
            let hdr = MediaHeader::from_slice(&hdr_bytes)?;
            let payload = recv.read_to_end(1024 * 1024).await?;
            Ok::<_, anyhow::Error>((hdr, payload))
        });
        keyframe_receivers.push(handle);
    }

    let idr_keyframe_data = vec![0x67u8; 32 * 1024]; // 32 KB dummy IDR keyframe
    let keyframe_send_start = Instant::now();

    // Streamer sends keyframe via Uni-Stream.
    // Intentionally send sender_peer_id = 0 to verify Requirement 3 (SFU rewrite).
    let mut streamer_uni_stream = clients[0].connection.open_uni().await?;
    let keyframe_header = MediaHeader::new_video_key(
        voice_channel_short_id,
        0, // Spoofed / dummy ID, must be rewritten by SFU to streamer_peer_id
        1,
        90_000,
        1,
        false,
    );
    streamer_uni_stream.write_all(&keyframe_header.to_bytes()).await?;
    streamer_uni_stream.write_all(&idr_keyframe_data).await?;
    streamer_uni_stream.finish()?;

    // Verify all 9 subscribers received the 32 KB keyframe and SFU rewrote sender_peer_id
    for (v_idx, rx_handle) in keyframe_receivers.into_iter().enumerate() {
        let (hdr, payload) = rx_handle.await??;

        if hdr.media_type != MediaType::VideoKey {
            return Err(LoadTestError::PacketIntegrityFailed(format!(
                "Viewer {v_idx} received non-VideoKey header: {:?}",
                hdr.media_type
            ))
            .into());
        }

        if hdr.sender_peer_id != streamer_peer_id {
            return Err(LoadTestError::SfuSenderRewriteFailed {
                expected: streamer_peer_id,
                actual: hdr.sender_peer_id,
            }
            .into());
        }

        if hdr.channel_id != voice_channel_short_id {
            return Err(LoadTestError::PacketIntegrityFailed(format!(
                "Viewer {v_idx} channel_id mismatch: got {}, expected {voice_channel_short_id}",
                hdr.channel_id
            ))
            .into());
        }

        if payload.len() != 32 * 1024 || payload != idr_keyframe_data {
            return Err(LoadTestError::PacketIntegrityFailed(format!(
                "Viewer {v_idx} keyframe payload corrupted: received {} bytes",
                payload.len()
            ))
            .into());
        }
    }
    let keyframe_delivery_time = keyframe_send_start.elapsed();

    // ─── 6. Prepare Datagram Receivers for Video Delta & Voice Load ─────────
    let all_stats: Vec<Arc<Mutex<ClientDatagramStats>>> = (0..CLIENT_COUNT)
        .map(|_| Arc::new(Mutex::new(ClientDatagramStats::default())))
        .collect();

    let mut datagram_receiver_handles = Vec::with_capacity(CLIENT_COUNT);
    let suite_epoch = Instant::now();

    for i in 0..CLIENT_COUNT {
        let conn = clients[i].connection.clone();
        let my_peer_id = clients[i].session_peer_id;
        let streamer_pid = streamer_peer_id;
        let ch_id = voice_channel_short_id;
        let stats_ref = all_stats[i].clone();

        let handle = tokio::spawn(async move {
            while let Ok(raw_bytes) = conn.read_datagram().await {
                let packet = match MediaPacket::parse(raw_bytes) {
                    Ok(p) => p,
                    Err(_) => {
                        let mut s = stats_ref.lock().await;
                        s.corrupted_count += 1;
                        continue;
                    }
                };

                let mut s = stats_ref.lock().await;

                if packet.header.channel_id != ch_id {
                    s.corrupted_count += 1;
                    continue;
                }

                match packet.header.media_type {
                    MediaType::Audio => {
                        // Participant must never receive own audio back
                        if packet.header.sender_peer_id == my_peer_id {
                            s.unauthorized_senders += 1;
                            continue;
                        }
                        if packet.payload.len() != 160 {
                            s.corrupted_count += 1;
                            continue;
                        }
                        s.audio_received += 1;
                        s.audio_senders.insert(packet.header.sender_peer_id);

                        if packet.payload.len() >= 8
                            && let Ok(ts_slice) = packet.payload[0..8].try_into()
                        {
                            let sent_us = u64::from_be_bytes(ts_slice);
                            let now_us = suite_epoch.elapsed().as_micros() as u64;
                            s.audio_latencies_us.push(now_us.saturating_sub(sent_us));
                        }
                    }
                    MediaType::VideoDelta => {
                        // Video delta must come strictly from the streamer
                        if packet.header.sender_peer_id != streamer_pid {
                            s.unauthorized_senders += 1;
                            continue;
                        }
                        s.video_delta_received += 1;
                        s.video_delta_fragments
                            .insert(packet.header.fragment_index, packet.payload);
                    }
                    _ => {
                        s.corrupted_count += 1;
                    }
                }
            }
        });
        datagram_receiver_handles.push(handle);
    }

    // ─── 7. Screen Share Video Delta Frame Fragmentation & Reassembly ───────
    // Delta frame size: 3480 bytes -> 3 fragments (1160 + 1160 + 1160)
    let delta_frame_payload = vec![0x42u8; 3480];
    let chunks: Vec<&[u8]> = delta_frame_payload.chunks(MAX_DATAGRAM_PAYLOAD).collect();
    let total_fragments = chunks.len() as u8;

    let delta_send_start = Instant::now();
    for (frag_idx, chunk) in chunks.iter().enumerate() {
        let seq = 500u16.wrapping_add(frag_idx as u16);
        // Intentionally set sender_peer_id = 0 to verify Requirement 3 (SFU rewrite)
        let header = MediaHeader::new_video_delta(
            voice_channel_short_id,
            0, // Spoofed ID, SFU must rewrite to streamer_peer_id
            seq,
            90_033,
            2, // frame_id
            frag_idx as u8,
            total_fragments,
            false,
        );
        let pkt = MediaPacket::new(header, Bytes::copy_from_slice(chunk));
        clients[0].connection.send_datagram(pkt.encode())?;
    }

    // Await reception of all 3 fragments on all 9 viewers
    let delta_wait_start = Instant::now();
    loop {
        let mut all_done = true;
        for viewer_stats in &all_stats[1..CLIENT_COUNT] {
            let s = viewer_stats.lock().await;
            if s.video_delta_fragments.len() < total_fragments as usize {
                all_done = false;
                break;
            }
        }
        if all_done {
            break;
        }
        if delta_wait_start.elapsed() > Duration::from_secs(3) {
            return Err(LoadTestError::Timeout(
                "Waiting for 9 viewers to receive all delta fragments".to_string(),
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let delta_delivery_time = delta_send_start.elapsed();

    // Verify delta reassembly and SFU sender rewrite on all 9 viewers
    for (v_idx, viewer_stats) in all_stats[1..CLIENT_COUNT].iter().enumerate() {
        let s = viewer_stats.lock().await;

        if s.corrupted_count > 0 {
            return Err(LoadTestError::PacketIntegrityFailed(format!(
                "Viewer {v_idx} recorded corrupted delta fragments"
            ))
            .into());
        }

        if s.unauthorized_senders > 0 {
            return Err(LoadTestError::SfuSenderRewriteFailed {
                expected: streamer_peer_id,
                actual: 0,
            }
            .into());
        }

        let mut reassembled = Vec::with_capacity(delta_frame_payload.len());
        for f in 0..total_fragments {
            let chunk_data = match s.video_delta_fragments.get(&f) {
                Some(b) => b,
                None => {
                    return Err(LoadTestError::PacketIntegrityFailed(format!(
                        "Viewer {v_idx} missing fragment {f}"
                    ))
                    .into());
                }
            };
            reassembled.extend_from_slice(chunk_data);
        }

        if reassembled != delta_frame_payload {
            return Err(LoadTestError::PacketIntegrityFailed(format!(
                "Viewer {v_idx} reassembled delta frame content mismatch"
            ))
            .into());
        }
    }

    // ─── 8. Voice Load Simulation: 10 Concurrent Clients at 50 pkts/s ───────
    // 10 clients concurrently transmit 100 synthetic Opus datagrams each (20 ms interval)
    // Total sent: 1,000 datagrams over ~2.0 seconds.
    // Total routed by SFU: 10 * 900 = 9,000 datagrams.
    const PACKETS_PER_CLIENT: u16 = 100;
    const TOTAL_AUDIO_SENT: u64 = (CLIENT_COUNT as u64) * (PACKETS_PER_CLIENT as u64);
    const EXPECTED_AUDIO_ROUTED: u64 = TOTAL_AUDIO_SENT * ((CLIENT_COUNT - 1) as u64);

    let voice_load_start = Instant::now();
    let mut sender_handles = Vec::with_capacity(CLIENT_COUNT);

    for (client_idx, client) in clients.iter().enumerate() {
        let conn = client.connection.clone();
        let session_peer_id = client.session_peer_id;
        let ch_id = voice_channel_short_id;

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(20));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);

            for seq_idx in 0..PACKETS_PER_CLIENT {
                interval.tick().await;

                let seq = 1000u16.wrapping_add(seq_idx);
                let timestamp = (seq_idx as u32 + 1) * 960; // 20 ms at 48000 Hz

                // 160-byte payload: 8B send timestamp, 4B client ID, 4B packet ID, 144B Opus dummy
                let mut payload = vec![0x55u8; 160];
                let now_us = suite_epoch.elapsed().as_micros() as u64;
                payload[0..8].copy_from_slice(&now_us.to_be_bytes());
                payload[8..12].copy_from_slice(&(client_idx as u32).to_be_bytes());
                payload[12..16].copy_from_slice(&(seq_idx as u32).to_be_bytes());

                let header = MediaHeader::new_audio(
                    ch_id,
                    session_peer_id,
                    seq,
                    timestamp,
                    false,
                    false,
                );

                let packet = MediaPacket::new(header, Bytes::from(payload));
                conn.send_datagram(packet.encode())?;
            }

            Ok::<_, anyhow::Error>(())
        });
        sender_handles.push(handle);
    }

    // Wait for all 10 sender tasks to complete
    for handle in sender_handles {
        handle.await??;
    }
    let send_finished = Instant::now();

    // Await delivery of all 9,000 routed packets (drain period with 1.5s timeout)
    loop {
        let mut total_received = 0u64;
        for s_mutex in &all_stats {
            let s = s_mutex.lock().await;
            total_received += s.audio_received;
        }

        if total_received >= EXPECTED_AUDIO_ROUTED || send_finished.elapsed() > Duration::from_millis(1500) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let voice_total_duration = voice_load_start.elapsed();

    // Terminate datagram receiver loops
    for handle in datagram_receiver_handles {
        handle.abort();
    }

    // ─── 9. Aggregate Statistics & Validation ────────────────────────────────
    let mut total_audio_received = 0u64;
    let mut total_corrupted = 0u64;
    let mut total_unauthorized = 0u64;
    let mut all_latencies: Vec<u64> = Vec::new();

    for (i, s_mutex) in all_stats.iter().enumerate() {
        let s = s_mutex.lock().await;
        total_audio_received += s.audio_received;
        total_corrupted += s.corrupted_count;
        total_unauthorized += s.unauthorized_senders;
        all_latencies.extend_from_slice(&s.audio_latencies_us);

        // Every participant must have received audio from all other 9 participants
        if s.audio_senders.len() != CLIENT_COUNT - 1 {
            return Err(LoadTestError::PacketIntegrityFailed(format!(
                "Client {i} received audio from {} peers, expected {}",
                s.audio_senders.len(),
                CLIENT_COUNT - 1
            ))
            .into());
        }
    }

    // Integrity Assertions
    if total_corrupted > 0 {
        return Err(LoadTestError::PacketIntegrityFailed(format!(
            "Total corrupted packets: {total_corrupted}"
        ))
        .into());
    }

    if total_unauthorized > 0 {
        return Err(LoadTestError::SfuSenderRewriteFailed {
            expected: 0,
            actual: total_unauthorized as u32,
        }
        .into());
    }

    let delivery_rate = (total_audio_received as f64) / (EXPECTED_AUDIO_ROUTED as f64) * 100.0;
    // Over loopback UDP, delivery rate must exceed 95%
    if total_audio_received < (EXPECTED_AUDIO_ROUTED * 95 / 100) {
        return Err(LoadTestError::DeliveryRateTooLow {
            received: total_audio_received,
            expected: EXPECTED_AUDIO_ROUTED,
            rate: delivery_rate,
        }
        .into());
    }

    // Latency Percentile Computations
    all_latencies.sort_unstable();
    let min_latency_us = match all_latencies.first() {
        Some(&v) => v,
        None => 0,
    };
    let max_latency_us = match all_latencies.last() {
        Some(&v) => v,
        None => 0,
    };
    let avg_latency_us = if !all_latencies.is_empty() {
        all_latencies.iter().sum::<u64>() / all_latencies.len() as u64
    } else {
        0
    };
    let p50_latency_us = match all_latencies.get(all_latencies.len() * 50 / 100) {
        Some(&v) => v,
        None => 0,
    };
    let p95_latency_us = match all_latencies.get(all_latencies.len() * 95 / 100) {
        Some(&v) => v,
        None => 0,
    };
    let p99_latency_us = match all_latencies.get(all_latencies.len() * 99 / 100) {
        Some(&v) => v,
        None => 0,
    };

    let total_audio_bytes = (total_audio_received as usize) * (MEDIA_HEADER_SIZE + 160);
    let duration_sec = voice_total_duration.as_secs_f64();
    let throughput_pkts_per_sec = (total_audio_received as f64) / duration_sec;
    let throughput_mbps = ((total_audio_bytes * 8) as f64) / duration_sec / 1_000_000.0;

    // ─── 10. Output Comprehensive Load Test Report ──────────────────────────
    println!("\n╔════════════════════════════════════════════════════════════════════════════╗");
    println!("║             FINCHSPEAK QUIC SFU LOAD TEST BENCHMARK RESULTS                ║");
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ Server Address:          {:<50}║", server_addr);
    println!("║ Active QUIC Clients:     {:<50}║", CLIENT_COUNT);
    println!("║ Voice Channel:           short_id={:<41}║", voice_channel_short_id);
    println!("║ Test Suite Elapsed:      {:<47?}║", test_suite_start.elapsed());
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ SCREEN SHARE METRICS (Uni-Stream IDR & Datagram Delta)                    ║");
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ IDR Keyframe Size:       32,768 bytes (Uni-Stream)                         ║");
    println!("║ IDR Subscribers:         9 viewers verified byte-for-byte                  ║");
    println!("║ IDR Delivery Time:       {:<47?}║", keyframe_delivery_time);
    println!("║ Delta Frame Size:        3,480 bytes (3 x 1160B Datagram fragments)        ║");
    println!("║ Delta Reassembly:        9 viewers reassembled 100% intact                 ║");
    println!("║ Delta Delivery Time:     {:<47?}║", delta_delivery_time);
    println!("║ SFU sender_peer_id:      PASSED (bytes 5..9 rewritten to sender session ID)║");
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ VOICE LOAD BENCHMARK (10 Concurrent Opus Audio Streams)                   ║");
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ Total Datagrams Sent:    {:<50}║", TOTAL_AUDIO_SENT);
    println!("║ Total Datagrams Routed:  {:<50}║", total_audio_received);
    println!("║ Delivery Rate:           {:<49.2}%║", delivery_rate);
    println!("║ Corrupted Packets:       {:<50}║", total_corrupted);
    println!("║ Spoofed Sender Leaks:    {:<50}║", total_unauthorized);
    println!("║ Total Audio Transferred: {:.2} MB ({total_audio_bytes} bytes)", (total_audio_bytes as f64) / (1024.0 * 1024.0));
    println!("║ Voice Benchmark Time:    {:<47?}║", voice_total_duration);
    println!("║ Throughput (pkts/sec):   {:<49.1} ║", throughput_pkts_per_sec);
    println!("║ Throughput (data rate):  {:<46.2} Mbps║", throughput_mbps);
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ DELIVERY LATENCY DISTRIBUTION (One-Way)                                    ║");
    println!("╠════════════════════════════════════════════════════════════════════════════╣");
    println!("║ Minimum Latency:         {:>8} µs ({:.3} ms)                             ║", min_latency_us, min_latency_us as f64 / 1000.0);
    println!("║ Average Latency:         {:>8} µs ({:.3} ms)                             ║", avg_latency_us, avg_latency_us as f64 / 1000.0);
    println!("║ Median Latency (P50):    {:>8} µs ({:.3} ms)                             ║", p50_latency_us, p50_latency_us as f64 / 1000.0);
    println!("║ 95th Percentile (P95):   {:>8} µs ({:.3} ms)                             ║", p95_latency_us, p95_latency_us as f64 / 1000.0);
    println!("║ 99th Percentile (P99):   {:>8} µs ({:.3} ms)                             ║", p99_latency_us, p99_latency_us as f64 / 1000.0);
    println!("║ Maximum Latency:         {:>8} µs ({:.3} ms)                             ║", max_latency_us, max_latency_us as f64 / 1000.0);
    println!("╚════════════════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
