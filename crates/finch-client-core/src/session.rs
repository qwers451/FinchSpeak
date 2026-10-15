//! QUIC session: authentication on the control stream and background I/O tasks.
//!
//! Everything received from the server is delivered as [`SessionEvent`]s on a
//! bounded channel. Media datagrams are dropped when the channel is full;
//! control messages, keyframe streams and disconnects are never dropped.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use finch_protocol::codec::{decode_message, encode_message, CodecError};
use finch_protocol::media::{MediaHeader, MEDIA_HEADER_SIZE};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::{ChannelMapping, ServerState};
use quinn::{Connection, Endpoint};
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::error::ClientError;
use crate::tofu::{create_client_config_with, TofuIssue, TofuIssueSlot};

/// Capacity of the session event queue.
pub const EVENT_QUEUE_CAPACITY: usize = 1024;

/// Maximum size of a keyframe received on a uni-directional stream.
const MAX_UNI_STREAM_BYTES: usize = 10 * 1024 * 1024;

/// Interval between keep-alive pings on the control stream.
const PING_INTERVAL: Duration = Duration::from_secs(4);

/// Where and how to connect.
#[derive(Debug, Clone)]
pub struct ConnectParams {
    pub quic_addr: SocketAddr,
    pub hostname: String,
    pub token: String,
    /// Reject servers whose fingerprint is not pinned yet (the UI asks the user) instead of
    /// pinning the first fingerprint seen.
    pub require_known_fingerprint: bool,
}

/// An authenticated QUIC session.
#[derive(Debug, Clone)]
pub struct Session {
    pub connection: Connection,
    pub control_tx: mpsc::Sender<ClientMessage>,
    pub user_id: Uuid,
    pub session_peer_id: u32,
    pub channels: Vec<ChannelMapping>,
    pub initial_state: ServerState,
}

/// Something received from the server.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Control stream message (including `Pong`).
    Control(ServerMessage),
    /// Raw media datagram (20-byte header + payload).
    Datagram(Bytes),
    /// Media received on a uni-directional stream (video keyframes).
    UniStream { header: MediaHeader, payload: Bytes },
    /// The control stream ended; the session is unusable.
    ConnectionLost(String),
}

impl Session {
    /// Send a raw media datagram.
    pub fn send_datagram(&self, packet: Bytes) -> Result<(), ClientError> {
        self.connection
            .send_datagram(packet)
            .map_err(|e| ClientError::Quic(e.to_string()))
    }
}

/// Connect, authenticate with `params.token`, and start the session I/O tasks.
pub async fn connect(
    params: ConnectParams,
    fingerprints: Arc<DashMap<String, String>>,
) -> Result<(Session, mpsc::Receiver<SessionEvent>), ClientError> {
    let issue: TofuIssueSlot = Arc::new(Mutex::new(None));
    let client_config = create_client_config_with(
        &params.hostname,
        fingerprints,
        params.require_known_fingerprint,
        issue.clone(),
    )?;

    let mut endpoint = Endpoint::client(SocketAddr::from(([0, 0, 0, 0], 0)))
        .map_err(|e| ClientError::Quic(e.to_string()))?;
    endpoint.set_default_client_config(client_config);

    info!("Connecting via QUIC to {} ({})", params.quic_addr, params.hostname);
    let connection = match endpoint
        .connect(params.quic_addr, &params.hostname)
        .map_err(|e| ClientError::Quic(e.to_string()))?
        .await
    {
        Ok(c) => c,
        Err(e) => {
            let reported = issue.lock().ok().and_then(|mut slot| slot.take());
            return Err(match reported {
                Some(TofuIssue::Unknown { fingerprint }) => ClientError::UntrustedServer {
                    hostname: params.hostname,
                    fingerprint,
                },
                Some(TofuIssue::Mismatch { expected, actual }) => ClientError::FingerprintMismatch {
                    hostname: params.hostname,
                    expected,
                    actual,
                },
                None => ClientError::Quic(e.to_string()),
            });
        }
    };
    info!("QUIC TLS handshake completed successfully");

    // Open bi-stream for Control Stream
    let (mut send_stream, mut recv_stream) = connection
        .open_bi()
        .await
        .map_err(|e| ClientError::Quic(e.to_string()))?;

    // Step 1: Send Authenticate message
    let encoded = encode_message(&ClientMessage::Authenticate { token: params.token })?;
    send_stream
        .write_all(&encoded)
        .await
        .map_err(|e| ClientError::Quic(e.to_string()))?;

    // Step 2: Receive Authenticated response
    let mut frame_buf = BytesMut::new();
    let mut read_buf = vec![0u8; 4096];
    let first_msg: ServerMessage = loop {
        let n = recv_stream
            .read(&mut read_buf)
            .await
            .map_err(|e| ClientError::Quic(e.to_string()))?
            .ok_or(ClientError::Closed)?;
        frame_buf.extend_from_slice(&read_buf[..n]);

        match decode_message::<ServerMessage>(&mut frame_buf) {
            Ok(msg) => break msg,
            Err(CodecError::Incomplete { .. }) => continue,
            Err(e) => return Err(e.into()),
        }
    };

    let (user_id, session_peer_id, channels, initial_state) = match first_msg {
        ServerMessage::Authenticated {
            user_id,
            session_peer_id,
            channels,
            initial_state,
        } => (user_id, session_peer_id, channels, initial_state),
        ServerMessage::Error { code, message } => {
            return Err(ClientError::AuthRejected { code, message });
        }
        other => return Err(ClientError::UnexpectedMessage(format!("{other:?}"))),
    };
    info!(
        "QUIC authenticated! Session peer ID: {}, User ID: {}",
        session_peer_id, user_id
    );

    let (event_tx, event_rx) = mpsc::channel::<SessionEvent>(EVENT_QUEUE_CAPACITY);
    let (control_tx, mut control_rx) = mpsc::channel::<ClientMessage>(128);

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
    let control_events = event_tx.clone();
    tokio::spawn(async move {
        let mut chunk_buf = vec![0u8; 4096];
        loop {
            match recv_stream.read(&mut chunk_buf).await {
                Ok(Some(n)) => {
                    frame_buf.extend_from_slice(&chunk_buf[..n]);
                    loop {
                        match decode_message::<ServerMessage>(&mut frame_buf) {
                            Ok(msg) => {
                                if control_events.send(SessionEvent::Control(msg)).await.is_err() {
                                    return;
                                }
                            }
                            Err(CodecError::Incomplete { .. }) => break,
                            Err(e) => {
                                error!("Decode error on control stream: {}", e);
                                break;
                            }
                        }
                    }
                }
                Ok(None) => {
                    info!("Control stream reached EOF");
                    let _ = control_events
                        .send(SessionEvent::ConnectionLost("Stream closed by server".to_string()))
                        .await;
                    break;
                }
                Err(e) => {
                    error!("Control stream read error: {}", e);
                    let _ = control_events.send(SessionEvent::ConnectionLost(e.to_string())).await;
                    break;
                }
            }
        }
    });

    // Task 3: Background Ping keepalive
    let ping_tx = control_tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(PING_INTERVAL).await;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if ping_tx.send(ClientMessage::Ping { timestamp: now }).await.is_err() {
                break;
            }
        }
    });

    // Task 4: Incoming Media Datagrams
    let conn_dgram = connection.clone();
    let dgram_events = event_tx.clone();
    tokio::spawn(async move {
        loop {
            match conn_dgram.read_datagram().await {
                Ok(bytes) => match dgram_events.try_send(SessionEvent::Datagram(bytes)) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        debug!("Session event queue full, dropping datagram");
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                },
                Err(e) => {
                    debug!("Datagram read loop ended: {}", e);
                    break;
                }
            }
        }
    });

    // Task 5: Incoming Uni-Streams (VideoKey frames)
    let conn_uni = connection.clone();
    let uni_events = event_tx;
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
                    if let Ok(payload) = recv.read_to_end(MAX_UNI_STREAM_BYTES).await {
                        let event = SessionEvent::UniStream {
                            header,
                            payload: Bytes::from(payload),
                        };
                        if uni_events.send(event).await.is_err() {
                            break;
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

    Ok((
        Session {
            connection,
            control_tx,
            user_id,
            session_peer_id,
            channels,
            initial_state,
        },
        event_rx,
    ))
}
