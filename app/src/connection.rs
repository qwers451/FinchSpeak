//! QUIC connection management, TOFU certificate verification, and packet routing.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::{Bytes, BytesMut};
use finch_protocol::codec::{decode_message, encode_message};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use quinn::{ClientConfig, Connection, Endpoint, TransportConfig};
use ring::digest::{digest, SHA256};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use tauri::Emitter;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::state::{AppState, VideoChunkPayload};

/// TOFU (Trust On First Use) Certificate Verifier.
#[derive(Debug)]
pub struct TofuVerifier {
    pub hostname: String,
    pub saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
    crypto_provider: Arc<rustls::crypto::CryptoProvider>,
}

impl TofuVerifier {
    pub fn new(hostname: String, saved_fingerprints: Arc<dashmap::DashMap<String, String>>) -> Self {
        Self {
            hostname,
            saved_fingerprints,
            crypto_provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }

    pub fn compute_fingerprint(cert: &[u8]) -> String {
        let hash = digest(&SHA256, cert);
        hash.as_ref()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(":")
    }
}

impl ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let cert_fp = Self::compute_fingerprint(end_entity.as_ref());

        if let Some(existing_fp) = self.saved_fingerprints.get(&self.hostname) {
            if existing_fp.value() != &cert_fp {
                error!(
                    "TOFU verification failed for {}: expected {}, received {}",
                    self.hostname, existing_fp.value(), cert_fp
                );
                return Err(RustlsError::InvalidCertificate(
                    rustls::CertificateError::ApplicationVerificationFailure,
                ));
            }
            debug!("TOFU certificate match for {}", self.hostname);
        } else {
            info!(
                "First connection to {}: trusting certificate fingerprint {}",
                self.hostname, cert_fp
            );
            self.saved_fingerprints.insert(self.hostname.clone(), cert_fp);
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.crypto_provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Create Quinn client configuration with TOFU verifier and RFC 9221 datagram settings.
pub fn create_client_config(
    hostname: &str,
    saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
) -> anyhow::Result<ClientConfig> {
    let verifier = Arc::new(TofuVerifier::new(hostname.to_string(), saved_fingerprints));

    let mut tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    tls_config.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];

    let quic_client_config = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)?;
    let mut client_config = ClientConfig::new(Arc::new(quic_client_config));

    let mut transport = TransportConfig::default();
    transport.max_idle_timeout(Some(Duration::from_secs(15).try_into()?));
    transport.keep_alive_interval(Some(Duration::from_secs(4)));
    transport.datagram_receive_buffer_size(Some(2 * 1024 * 1024));
    transport.datagram_send_buffer_size(2 * 1024 * 1024);
    client_config.transport_config(Arc::new(transport));

    Ok(client_config)
}

/// Establish QUIC connection and spawn message loops.
pub async fn start_quic_session(
    server_addr: SocketAddr,
    hostname: &str,
    token: String,
    state: AppState,
    app_handle: tauri::AppHandle,
) -> anyhow::Result<Connection> {
    let client_config = create_client_config(hostname, state.saved_fingerprints.clone())?;

    let mut endpoint = Endpoint::client(SocketAddr::from(([0, 0, 0, 0], 0)))?;
    endpoint.set_default_client_config(client_config);

    info!("Connecting via QUIC to {} ({})", server_addr, hostname);
    let connection = endpoint.connect(server_addr, hostname)?.await?;
    info!("QUIC TLS handshake completed successfully");

    // Open bi-stream for Control Stream
    let (mut send_stream, mut recv_stream) = connection.open_bi().await?;

    // Step 1: Send Authenticate message
    let auth_msg = ClientMessage::Authenticate { token };
    let encoded = encode_message(&auth_msg)?;
    send_stream.write_all(&encoded).await?;

    // Step 2: Receive Authenticated response
    let mut frame_buf = BytesMut::new();
    let mut read_buf = vec![0u8; 4096];

    let first_msg: ServerMessage = loop {
        let n = recv_stream
            .read(&mut read_buf)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream closed before authentication response"))?;
        frame_buf.extend_from_slice(&read_buf[..n]);

        match decode_message::<ServerMessage>(&mut frame_buf) {
            Ok(msg) => break msg,
            Err(finch_protocol::codec::CodecError::Incomplete { .. }) => continue,
            Err(e) => return Err(e.into()),
        }
    };

    match first_msg {
        ServerMessage::Authenticated {
            user_id,
            session_peer_id,
            channels,
            initial_state,
        } => {
            info!(
                "QUIC authenticated! Session peer ID: {}, User ID: {}",
                session_peer_id, user_id
            );
            state.session_peer_id.store(session_peer_id, Ordering::SeqCst);
            let _ = app_handle.emit(
                "authenticated",
                serde_json::json!({
                    "session_peer_id": session_peer_id,
                    "user_id": user_id,
                    "channels": channels,
                    "initial_state": initial_state
                }),
            );
        }
        ServerMessage::Error { code, message } => {
            return Err(anyhow::anyhow!("QUIC authentication failed ({}): {}", code, message));
        }
        other => {
            return Err(anyhow::anyhow!("Unexpected first message: {:?}", other));
        }
    }

    // Channel for queuing outgoing client messages
    let (control_tx, mut control_rx) = mpsc::channel::<ClientMessage>(128);
    *state.control_tx.write().await = Some(control_tx.clone());
    *state.quic_connection.write().await = Some(connection.clone());

    // Task 1: Outgoing Control Stream Sender
    tokio::spawn(async move {
        while let Some(msg) = control_rx.recv().await {
            match encode_message(&msg) {
                Ok(bytes) => {
                    if let Err(e) = send_stream.write_all(&bytes).await {
                        error!("Failed to write control message: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to encode control message: {}", e);
                }
            }
        }
        debug!("Control send loop terminated");
    });

    // Task 2: Incoming Control Stream Receiver
    let app_handle_clone = app_handle.clone();
    tokio::spawn(async move {
        let mut loop_buf = BytesMut::new();
        let mut chunk_buf = vec![0u8; 4096];

        loop {
            match recv_stream.read(&mut chunk_buf).await {
                Ok(Some(n)) => {
                    loop_buf.extend_from_slice(&chunk_buf[..n]);

                    loop {
                        match decode_message::<ServerMessage>(&mut loop_buf) {
                            Ok(msg) => match msg {
                                ServerMessage::NewMessage {
                                    message_id,
                                    channel_id,
                                    author_id,
                                    author_name,
                                    content,
                                    attachments,
                                    timestamp,
                                } => {
                                    let _ = app_handle_clone.emit(
                                        "new-message",
                                        serde_json::json!({
                                            "message_id": message_id,
                                            "channel_id": channel_id,
                                            "author_id": author_id,
                                            "author_name": author_name,
                                            "content": content,
                                            "attachments": attachments,
                                            "timestamp": timestamp,
                                        }),
                                    );
                                }
                                ServerMessage::MessageHistory {
                                    channel_id,
                                    messages,
                                    has_more,
                                } => {
                                    let _ = app_handle_clone.emit(
                                        "message-history",
                                        serde_json::json!({
                                            "channel_id": channel_id,
                                            "messages": messages,
                                            "has_more": has_more,
                                        }),
                                    );
                                }
                                ServerMessage::UserJoinedVoice {
                                    user_id,
                                    peer_id,
                                    username,
                                    channel_id,
                                } => {
                                    let _ = app_handle_clone.emit(
                                        "user-joined-voice",
                                        serde_json::json!({
                                            "user_id": user_id,
                                            "peer_id": peer_id,
                                            "username": username,
                                            "channel_id": channel_id,
                                        }),
                                    );
                                }
                                ServerMessage::UserLeftVoice {
                                    user_id,
                                    peer_id,
                                    channel_id,
                                } => {
                                    let _ = app_handle_clone.emit(
                                        "user-left-voice",
                                        serde_json::json!({
                                            "user_id": user_id,
                                            "peer_id": peer_id,
                                            "channel_id": channel_id,
                                        }),
                                    );
                                }
                                ServerMessage::UserVoiceStateChanged {
                                    peer_id,
                                    muted,
                                    deafened,
                                } => {
                                    let _ = app_handle_clone.emit(
                                        "user-voice-state-changed",
                                        serde_json::json!({
                                            "peer_id": peer_id,
                                            "muted": muted,
                                            "deafened": deafened,
                                        }),
                                    );
                                }
                                ServerMessage::ScreenShareStarted {
                                    user_id,
                                    peer_id,
                                    codec,
                                    width,
                                    height,
                                    fps,
                                } => {
                                    let _ = app_handle_clone.emit(
                                        "screen-share-started",
                                        serde_json::json!({
                                            "user_id": user_id,
                                            "peer_id": peer_id,
                                            "codec": codec,
                                            "width": width,
                                            "height": height,
                                            "fps": fps,
                                        }),
                                    );
                                }
                                ServerMessage::ScreenShareStopped { peer_id } => {
                                    let _ = app_handle_clone.emit(
                                        "screen-share-stopped",
                                        serde_json::json!({ "peer_id": peer_id }),
                                    );
                                }
                                ServerMessage::PresenceUpdate { user_id, status } => {
                                    let _ = app_handle_clone.emit(
                                        "presence-update",
                                        serde_json::json!({
                                            "user_id": user_id,
                                            "status": status,
                                        }),
                                    );
                                }
                                ServerMessage::KeyframeRequested => {
                                    let _ = app_handle_clone.emit("keyframe-requested", ());
                                }
                                ServerMessage::Pong { timestamp } => {
                                    let now = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as u64;
                                    let rtt = now.saturating_sub(timestamp);
                                    let _ = app_handle_clone.emit(
                                        "ping-rtt",
                                        serde_json::json!({ "rtt_ms": rtt }),
                                    );
                                }
                                ServerMessage::Error { code, message } => {
                                    warn!("Server error message (code {}): {}", code, message);
                                    let _ = app_handle_clone.emit(
                                        "server-error",
                                        serde_json::json!({ "code": code, "message": message }),
                                    );
                                }
                                _ => {}
                            },
                            Err(finch_protocol::codec::CodecError::Incomplete { .. }) => break,
                            Err(e) => {
                                error!("Decode error on control stream: {}", e);
                                break;
                            }
                        }
                    }
                }
                Ok(None) => {
                    info!("Control stream reached EOF");
                    let _ = app_handle_clone.emit("connection-lost", "Stream closed by server");
                    break;
                }
                Err(e) => {
                    error!("Control stream read error: {}", e);
                    let _ = app_handle_clone.emit("connection-lost", e.to_string());
                    break;
                }
            }
        }
    });

    // Task 3: Background Ping keepalive
    let ping_tx = control_tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(4)).await;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if ping_tx.send(ClientMessage::Ping { timestamp: now }).await.is_err() {
                break;
            }
        }
    });

    // Task 4: Incoming Media Datagrams Router (Audio + VideoDelta)
    let conn_dgram = connection.clone();
    let state_dgram = state.clone();
    let app_handle_clone_dgram = app_handle.clone();
    tokio::spawn(async move {
        loop {
            match conn_dgram.read_datagram().await {
                Ok(bytes) => {
                    if bytes.len() < MEDIA_HEADER_SIZE {
                        continue;
                    }
                    let header = match MediaHeader::from_slice(&bytes[..MEDIA_HEADER_SIZE]) {
                        Ok(h) => h,
                        Err(_) => continue,
                    };
                    let payload = &bytes[MEDIA_HEADER_SIZE..];

                    match header.media_type {
                        MediaType::Audio => {
                            if !state_dgram.is_deafened.load(Ordering::Relaxed) {
                                let mut mixer = state_dgram.mixer.lock().await;
                                if let Some((peer_id, speaking)) = mixer.on_audio_packet(
                                    &header,
                                    Bytes::copy_from_slice(payload),
                                    Instant::now(),
                                ) {
                                    let _ = app_handle_clone_dgram.emit(
                                        "user-speaking",
                                        serde_json::json!({
                                            "peer_id": peer_id,
                                            "is_speaking": speaking
                                        }),
                                    );
                                }
                            }
                        }
                        MediaType::VideoDelta => {
                            let mut asm = state_dgram.assembler.lock().await;
                            if let Some(frame) = asm.push_fragment(
                                &header,
                                Bytes::copy_from_slice(payload),
                                Instant::now(),
                            ) {
                                if let Some(ch) = &*state_dgram.video_channel.read().await {
                                    let _ = ch.send(VideoChunkPayload {
                                        is_key: false,
                                        data: frame.data.to_vec(),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    debug!("Datagram read loop ended: {}", e);
                    break;
                }
            }
        }
    });

    // Task 5: Incoming Uni-Streams Receiver (VideoKey frames)
    let conn_uni = connection.clone();
    let state_uni = state.clone();
    tokio::spawn(async move {
        loop {
            match conn_uni.accept_uni().await {
                Ok(mut recv) => {
                    let mut hdr_bytes = [0u8; MEDIA_HEADER_SIZE];
                    if recv.read_exact(&mut hdr_bytes).await.is_err() {
                        continue;
                    }
                    let header = match MediaHeader::from_slice(&hdr_bytes) {
                        Ok(h) => h,
                        Err(_) => continue,
                    };

                    if header.media_type == MediaType::VideoKey {
                        if let Ok(payload) = recv.read_to_end(10 * 1024 * 1024).await {
                            let mut asm = state_uni.assembler.lock().await;
                            let frame = asm.on_keyframe_received(
                                header.frame_id,
                                header.timestamp,
                                Bytes::from(payload),
                            );
                            if let Some(ch) = &*state_uni.video_channel.read().await {
                                let _ = ch.send(VideoChunkPayload {
                                    is_key: true,
                                    data: frame.data.to_vec(),
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!("Uni-stream receiver ended: {}", e);
                    break;
                }
            }
        }
    });

    Ok(connection)
}
