//! QUIC listener, connection lifecycle, signaling stream, and media routing.

use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use quinn::crypto::rustls::QuicServerConfig;
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use uuid::Uuid;

use finch_protocol::codec::{decode_message, encode_message};
use finch_protocol::media::{MediaHeader, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::{
    Channel, ChannelMapping, ChannelType, ServerState, User, UserRole, UserStatus, VoiceState,
};

use crate::api::auth::verify_jwt;
use crate::session::{ClientSession, SessionManager};
use crate::sfu::SfuRouter;

/// Generate a self-signed TLS certificate for the QUIC server.
pub fn generate_self_signed_cert() -> Result<(rustls::pki_types::CertificateDer<'static>, rustls::pki_types::PrivateKeyDer<'static>)> {
    let cert_params = rcgen::CertificateParams::new(vec!["localhost".to_string()])?;
    let key_pair = rcgen::KeyPair::generate()?;
    let cert = cert_params.self_signed(&key_pair)?;

    let cert_der = rustls::pki_types::CertificateDer::from(cert);
    let key_der = rustls::pki_types::PrivateKeyDer::try_from(key_pair.serialize_der())
        .map_err(|e| anyhow::anyhow!("failed to parse private key: {e}"))?;

    Ok((cert_der, key_der))
}

/// Load the server certificate and key from `dir` (`cert.der`, `key.der`), creating and
/// storing a new self-signed pair on first start so the TOFU fingerprint stays stable.
pub fn load_or_create_cert(
    dir: &std::path::Path,
) -> Result<(rustls::pki_types::CertificateDer<'static>, rustls::pki_types::PrivateKeyDer<'static>)> {
    let cert_path = dir.join("cert.der");
    let key_path = dir.join("key.der");

    if cert_path.exists() && key_path.exists() {
        let cert = std::fs::read(&cert_path).with_context(|| format!("Failed to read {cert_path:?}"))?;
        let key = std::fs::read(&key_path).with_context(|| format!("Failed to read {key_path:?}"))?;
        let key = rustls::pki_types::PrivateKeyDer::try_from(key)
            .map_err(|e| anyhow::anyhow!("failed to parse private key {key_path:?}: {e}"))?;
        return Ok((rustls::pki_types::CertificateDer::from(cert), key));
    }

    let (cert, key) = generate_self_signed_cert()?;
    std::fs::create_dir_all(dir).with_context(|| format!("Failed to create {dir:?}"))?;
    std::fs::write(&cert_path, cert.as_ref()).with_context(|| format!("Failed to write {cert_path:?}"))?;
    std::fs::write(&key_path, key.secret_der()).with_context(|| format!("Failed to write {key_path:?}"))?;
    tracing::info!("Generated a new TLS certificate in {dir:?}");
    Ok((cert, key))
}

/// Build a quinn server config with a fresh self-signed certificate (tests).
pub fn make_server_config() -> Result<quinn::ServerConfig> {
    let (cert, key) = generate_self_signed_cert()?;
    make_server_config_with(cert, key)
}

/// Build a quinn server config with TLS 1.3 and transport settings.
pub fn make_server_config_with(
    cert: rustls::pki_types::CertificateDer<'static>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> Result<quinn::ServerConfig> {

    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)?;

    tls_config.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];

    let quic_config = QuicServerConfig::try_from(tls_config)?;
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_config));

    let mut transport_config = quinn::TransportConfig::default();
    transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(15).try_into()?));
    transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(4)));
    transport_config.max_concurrent_bidi_streams(16u32.into());
    transport_config.max_concurrent_uni_streams(256u32.into());
    transport_config.datagram_receive_buffer_size(Some(2 * 1024 * 1024));
    transport_config.datagram_send_buffer_size(2 * 1024 * 1024);
    server_config.transport_config(Arc::new(transport_config));

    // Log SHA-256 fingerprint for TOFU
    let fingerprint = ring::digest::digest(&ring::digest::SHA256, cert.as_ref());
    let hex: String = fingerprint
        .as_ref()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    tracing::info!("TLS certificate fingerprint (SHA-256): {hex}");

    Ok(server_config)
}

/// Run the QUIC server accept loop on an already bound endpoint.
pub async fn run_server(
    endpoint: quinn::Endpoint,
    pool: SqlitePool,
    sessions: SessionManager,
    jwt_secret: String,
) -> Result<()> {
    // Cache database channels into SessionManager
    sync_channels_from_db(&pool, &sessions).await?;

    let sfu = SfuRouter::new(sessions.clone());
    tracing::info!("QUIC server listening on {}", endpoint.local_addr()?);

    while let Some(incoming) = endpoint.accept().await {
        let pool = pool.clone();
        let sessions = sessions.clone();
        let sfu = sfu.clone();
        let jwt_secret = jwt_secret.clone();

        // Each handshake runs in its own task: a failed handshake must never stop the accept loop.
        tokio::spawn(async move {
            let remote = incoming.remote_address();
            let connection = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("QUIC handshake from {remote} failed: {e}");
                    return;
                }
            };
            tracing::info!("New QUIC connection from {remote}");
            if let Err(e) = handle_connection(connection, pool, sessions, sfu, jwt_secret).await {
                tracing::warn!("Connection from {remote} terminated: {e}");
            } else {
                tracing::info!("Connection from {remote} closed cleanly");
            }
        });
    }

    Ok(())
}

/// Synchronize channel IDs and short_ids from SQLite into the SessionManager.
pub async fn sync_channels_from_db(pool: &SqlitePool, sessions: &SessionManager) -> Result<()> {
    let rows = sqlx::query_as::<_, (String, i64)>("SELECT id, short_id FROM channels")
        .fetch_all(pool)
        .await?;

    for (id_str, short_id) in rows {
        if let Ok(uuid) = Uuid::parse_str(&id_str) {
            sessions.register_channel(uuid, short_id as u32);
        }
    }
    Ok(())
}

/// Handle a single authenticated QUIC connection.
pub async fn handle_connection(
    conn: quinn::Connection,
    pool: SqlitePool,
    sessions: SessionManager,
    sfu: SfuRouter,
    jwt_secret: String,
) -> Result<()> {
    // 1. Accept first bi-directional stream (Control Stream)
    let (mut send_stream, mut recv_stream) = conn.accept_bi().await?;
    let mut frame_buf = BytesMut::new();
    let mut read_buf = vec![0u8; 4096];

    // 2. Wait for Authenticate message
    let auth_msg = loop {
        let n = recv_stream
            .read(&mut read_buf)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Stream closed before authentication"))?;

        frame_buf.extend_from_slice(&read_buf[..n]);

        match decode_message::<ClientMessage>(&mut frame_buf) {
            Ok(ClientMessage::Authenticate { token }) => break token,
            Ok(_) => {
                let err = ServerMessage::Error {
                    code: 401,
                    message: "First message must be Authenticate".to_string(),
                };
                send_stream.write_all(&encode_message(&err)?).await?;
                return Err(anyhow::anyhow!("Unauthenticated first message"));
            }
            Err(finch_protocol::codec::CodecError::Incomplete { .. }) => continue,
            Err(e) => return Err(e.into()),
        }
    };

    // 3. Verify JWT
    let claims = match verify_jwt(&auth_msg, &jwt_secret) {
        Ok(c) => c,
        Err(_) => {
            let err = ServerMessage::Error {
                code: 401,
                message: "Invalid or expired JWT token".to_string(),
            };
            send_stream.write_all(&encode_message(&err)?).await?;
            return Err(anyhow::anyhow!("Invalid JWT token"));
        }
    };

    let user_id = Uuid::parse_str(&claims.sub)?;
    let peer_id = sessions.allocate_peer_id();

    // 4. Setup outbound channel for Control Stream messages
    let (control_tx, mut control_rx) = mpsc::channel::<ServerMessage>(128);

    // 5. Build Initial Server State
    let channels_db = sqlx::query_as::<_, (String, i64, String, String, i64, Option<String>, i64)>(
        "SELECT id, short_id, name, channel_type, position, topic, bitrate_kbps FROM channels ORDER BY position ASC"
    )
    .fetch_all(&pool)
    .await?;

    let mut channels = Vec::new();
    let mut channel_mappings = Vec::new();

    for (c_id, short_id, name, c_type, pos, topic, bitrate) in channels_db {
        if let Ok(c_uuid) = Uuid::parse_str(&c_id) {
            let c_enum = if c_type == "voice" {
                ChannelType::Voice
            } else {
                ChannelType::Text
            };
            channels.push(Channel {
                id: c_uuid,
                short_id: short_id as u32,
                name,
                channel_type: c_enum,
                topic,
                position: pos as i32,
                bitrate_kbps: bitrate as u32,
            });
            channel_mappings.push(ChannelMapping {
                id: c_uuid,
                short_id: short_id as u32,
            });
            sessions.register_channel(c_uuid, short_id as u32);
        }
    }

    let mut online_users = Vec::new();
    let mut voice_states = Vec::new();

    for entry in sessions.sessions.iter() {
        let s = entry.value();
        online_users.push(User {
            id: s.user_id,
            username: s.username.clone(),
            display_name: s.username.clone(),
            avatar_url: None,
            status: UserStatus::Online,
            role: if s.role == "admin" { UserRole::Admin } else { UserRole::Member },
        });

        if let Some(short_id) = s.current_voice_channel
            && let Some(c_uuid) = sessions.short_to_channel.get(&short_id)
        {
            voice_states.push(VoiceState {
                user_id: s.user_id,
                peer_id: s.peer_id,
                username: s.username.clone(),
                channel_id: *c_uuid,
                muted: s.muted,
                deafened: s.deafened,
                sharing_screen: false,
            });
        }
    }

    let initial_state = ServerState {
        channels,
        online_users,
        voice_states,
    };

    // 6. Send Authenticated response
    let auth_ack = ServerMessage::Authenticated {
        user_id,
        session_peer_id: peer_id,
        channels: channel_mappings,
        initial_state,
    };
    send_stream.write_all(&encode_message(&auth_ack)?).await?;

    // 7. Register session and broadcast PresenceUpdate
    let session = ClientSession {
        peer_id,
        user_id,
        username: claims.username.clone(),
        role: claims.role.clone(),
        current_voice_channel: None,
        muted: false,
        deafened: false,
        connection: conn.clone(),
        control_tx: control_tx.clone(),
    };
    sessions.register_session(session);

    sessions
        .broadcast_all(ServerMessage::PresenceUpdate {
            user_id,
            status: UserStatus::Online,
        })
        .await;

    tracing::info!("User '{}' ({user_id}) registered with session_peer_id={peer_id}", claims.username);

    // 8. Run concurrent loops for control send, control receive, datagrams, and uni-streams
    let conn_datagram = conn.clone();
    let conn_uni = conn.clone();
    let sfu_datagram = sfu.clone();
    let sfu_uni = sfu.clone();

    // Outbound control writer
    let mut writer_send_stream = send_stream;
    let control_writer_task = tokio::spawn(async move {
        while let Some(msg) = control_rx.recv().await {
            if let Ok(encoded) = encode_message(&msg)
                && writer_send_stream.write_all(&encoded).await.is_err()
            {
                break;
            }
        }
    });

    // Inbound control reader
    let pool_reader = pool.clone();
    let sessions_reader = sessions.clone();
    let username_reader = claims.username.clone();

    let control_reader_task = tokio::spawn(async move {
        loop {
            let n = match recv_stream.read(&mut read_buf).await {
                Ok(Some(n)) => n,
                _ => break,
            };

            frame_buf.extend_from_slice(&read_buf[..n]);

            loop {
                match decode_message::<ClientMessage>(&mut frame_buf) {
                    Ok(client_msg) => {
                        handle_client_message(
                            client_msg,
                            peer_id,
                            user_id,
                            &username_reader,
                            &pool_reader,
                            &sessions_reader,
                        )
                        .await;
                    }
                    Err(finch_protocol::codec::CodecError::Incomplete { .. }) => break,
                    Err(e) => {
                        tracing::warn!("Codec error on control stream from peer {peer_id}: {e}");
                        return;
                    }
                }
            }
        }
    });

    // Datagram forwarder
    let datagram_task = tokio::spawn(async move {
        while let Ok(data) = conn_datagram.read_datagram().await {
            sfu_datagram.route_datagram(&data, peer_id).await;
        }
    });

    // Uni-Stream forwarder
    let uni_task = tokio::spawn(async move {
        while let Ok(mut recv) = conn_uni.accept_uni().await {
            let mut hdr_bytes = [0u8; MEDIA_HEADER_SIZE];
            if recv.read_exact(&mut hdr_bytes).await.is_ok()
                && let Ok(hdr) = MediaHeader::from_slice(&hdr_bytes)
                && let Ok(payload) = recv.read_to_end(1024 * 1024).await
            {
                sfu_uni
                    .route_uni_keyframe(hdr.channel_id, peer_id, &hdr, Bytes::from(payload))
                    .await;
            }
        }
    });

    // Wait until any task finishes (disconnection)
    tokio::select! {
        _ = control_reader_task => {},
        _ = datagram_task => {},
        _ = uni_task => {},
    }

    // Cleanup upon disconnect
    control_writer_task.abort();
    let left_voice = sessions
        .sessions
        .get(&peer_id)
        .and_then(|s| s.current_voice_channel)
        .and_then(|short_id| sessions.short_to_channel.get(&short_id).map(|c| *c));
    sessions.unregister_session(peer_id);
    if let Some(channel_id) = left_voice {
        sessions
            .broadcast_all(ServerMessage::UserLeftVoice {
                user_id,
                peer_id,
                channel_id,
            })
            .await;
    }
    sessions
        .broadcast_all(ServerMessage::PresenceUpdate {
            user_id,
            status: UserStatus::Offline,
        })
        .await;

    tracing::info!("User '{}' ({user_id}) session {peer_id} disconnected", claims.username);

    Ok(())
}

/// Handle a single incoming ClientMessage on the Control Stream.
async fn handle_client_message(
    msg: ClientMessage,
    peer_id: u32,
    user_id: Uuid,
    username: &str,
    pool: &SqlitePool,
    sessions: &SessionManager,
) {
    match msg {
        ClientMessage::Ping { timestamp } => {
            // Clone the sender so no DashMap guard is held across the `.await`.
            let tx = sessions.sessions.get(&peer_id).map(|s| s.control_tx.clone());
            if let Some(tx) = tx {
                let _ = tx.send(ServerMessage::Pong { timestamp }).await;
            }
        }

        ClientMessage::SendMessage {
            channel_id,
            content,
            attachments,
        } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            // Generate UUID v7 for time-sorted message ID
            let message_id = Uuid::now_v7();

            let res = sqlx::query(
                "INSERT INTO messages (id, channel_id, author_id, content, created_at) VALUES (?, ?, ?, ?, ?)"
            )
            .bind(message_id.to_string())
            .bind(channel_id.to_string())
            .bind(user_id.to_string())
            .bind(&content)
            .bind(now)
            .execute(pool)
            .await;

            if res.is_ok() {
                // Link attachments to this message
                for att_id in &attachments {
                    let _ = sqlx::query("UPDATE attachments SET message_id = ? WHERE id = ?")
                        .bind(message_id.to_string())
                        .bind(att_id.to_string())
                        .execute(pool)
                        .await;
                }

                // Broadcast NewMessage to all connected users
                let new_msg = ServerMessage::NewMessage {
                    message_id,
                    channel_id,
                    author_id: user_id,
                    author_name: username.to_string(),
                    content,
                    attachments: vec![],
                    timestamp: now,
                };
                sessions.broadcast_all(new_msg).await;
            }
        }

        ClientMessage::JoinVoice { channel_id } => {
            if let Some(short_id) = sessions.channel_to_short.get(&channel_id) {
                sessions.join_voice(peer_id, *short_id);

                let event = ServerMessage::UserJoinedVoice {
                    user_id,
                    peer_id,
                    username: username.to_string(),
                    channel_id,
                };
                sessions.broadcast_all(event).await;
            }
        }

        ClientMessage::LeaveVoice => {
            let voice_info = if let Some(session) = sessions.sessions.get(&peer_id) {
                session.current_voice_channel.and_then(|short_id| {
                    sessions.short_to_channel.get(&short_id).map(|c_uuid| (short_id, *c_uuid))
                })
            } else {
                None
            };

            if let Some((short_id, c_id)) = voice_info {
                sessions.leave_voice(peer_id, short_id);

                let event = ServerMessage::UserLeftVoice {
                    user_id,
                    peer_id,
                    channel_id: c_id,
                };
                sessions.broadcast_all(event).await;
            }
        }

        ClientMessage::UpdateVoiceState { muted, deafened } => {
            let updated = if let Some(mut session) = sessions.sessions.get_mut(&peer_id) {
                session.muted = muted;
                session.deafened = deafened;
                true
            } else {
                false
            };

            if updated {
                let event = ServerMessage::UserVoiceStateChanged {
                    peer_id,
                    muted,
                    deafened,
                };
                sessions.broadcast_all(event).await;
            }
        }

        ClientMessage::StartScreenShare {
            codec,
            width,
            height,
            fps,
        } => {
            let short_id = sessions.sessions.get(&peer_id).and_then(|s| s.current_voice_channel);
            if let Some(short_id) = short_id {
                let event = ServerMessage::ScreenShareStarted {
                    user_id,
                    peer_id,
                    codec,
                    width,
                    height,
                    fps,
                };
                sessions.broadcast_voice(short_id, event, Some(peer_id)).await;
            }
        }

        ClientMessage::StopScreenShare => {
            let short_id = sessions.sessions.get(&peer_id).and_then(|s| s.current_voice_channel);
            if let Some(short_id) = short_id {
                let event = ServerMessage::ScreenShareStopped { peer_id };
                sessions.broadcast_voice(short_id, event, Some(peer_id)).await;
            }
        }

        ClientMessage::SubscribeScreenShare { target_user_id } => {
            let target_peer = sessions.user_to_peer.get(&target_user_id).map(|p| *p);
            if let Some(target_peer) = target_peer {
                sessions.subscribe_screen(peer_id, target_peer);
            }
        }

        ClientMessage::UnsubscribeScreenShare { target_user_id } => {
            if let Some(target_peer) = sessions.user_to_peer.get(&target_user_id) {
                sessions.unsubscribe_screen(peer_id, *target_peer);
            }
        }

        ClientMessage::RequestKeyframe { target_peer_id } => {
            let tx = sessions.sessions.get(&target_peer_id).map(|s| s.control_tx.clone());
            if let Some(tx) = tx {
                let _ = tx.send(ServerMessage::KeyframeRequested).await;
            }
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The certificate must survive restarts, otherwise every client's pinned
    /// fingerprint breaks on each server start (AUD-005).
    #[test]
    fn certificate_is_persisted_across_restarts() {
        let dir = std::env::temp_dir().join(format!("finch-tls-{}", Uuid::new_v4()));
        let (cert_first, _) = load_or_create_cert(&dir).unwrap();
        let (cert_second, _) = load_or_create_cert(&dir).unwrap();
        assert_eq!(cert_first.as_ref(), cert_second.as_ref());
        let _ = std::fs::remove_dir_all(dir);
    }
}
