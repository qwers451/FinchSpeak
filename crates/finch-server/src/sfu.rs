//! Selective Forwarding Unit (SFU) router.
//!
//! Validates incoming datagrams and uni-streams, checks room membership,
//! rewrites `sender_peer_id` to prevent spoofing, and fans out media to subscribers.

use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use bytes::{Bytes, BytesMut};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};

use crate::health::TRAFFIC;
use crate::limits::MediaRateLimiter;
use crate::session::SessionManager;

/// Hand one datagram to a receiver and count it: bytes out, or a drop when it could not be sent.
fn forward_datagram(connection: &quinn::Connection, datagram: Bytes) {
    let len = datagram.len() as u64;
    if connection.send_datagram(datagram).is_ok() {
        TRAFFIC.bytes_out.fetch_add(len, Ordering::Relaxed);
    } else {
        TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
    }
}

/// Charge `bytes` to a participant's media budget; `false` when it is over the limit.
fn admit(budget: &Mutex<MediaRateLimiter>, bytes: usize) -> bool {
    budget
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .allow(Instant::now(), bytes)
}

/// SFU Media Router.
#[derive(Clone)]
pub struct SfuRouter {
    sessions: SessionManager,
}

impl SfuRouter {
    pub fn new(sessions: SessionManager) -> Self {
        Self { sessions }
    }

    /// Route an incoming media datagram. Taken by value so the sender-id rewrite can reuse its buffer.
    pub async fn route_datagram(&self, datagram: Bytes, auth_peer_id: u32) {
        TRAFFIC.bytes_in.fetch_add(datagram.len() as u64, Ordering::Relaxed);

        // The participant's media budget (bytes and datagrams per second) is charged for every
        // datagram it sends, malformed ones included, before anything is routed.
        let (voice_channel, sending_muted, budget) = match self.sessions.sessions.get(&auth_peer_id) {
            Some(s) => (s.current_voice_channel, s.muted || s.server_muted, s.media.clone()),
            None => {
                TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        if !admit(&budget, datagram.len()) {
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let header = match MediaHeader::from_slice(&datagram) {
            Ok(h) => h,
            Err(_) => {
                // Too short, or an unknown media type: there is nothing to route.
                TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // 1. Validate that the sender is in the room specified in the header
        if voice_channel != Some(header.channel_id) {
            tracing::warn!(
                "Peer {auth_peer_id} attempted to send to channel {} but is in {:?}",
                header.channel_id,
                voice_channel
            );
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // A muted microphone sends no voice; the sound of a shared screen is a separate stream.
        if sending_muted && header.media_type == MediaType::Audio {
            return;
        }

        // C4 hook: check here that the sender may send this media type. Screen video and screen
        // audio only while the sender has a screen share started. A refused datagram is dropped
        // with `TRAFFIC.datagrams_dropped` counted, like the cases above.

        // 2. Rewrite sender_peer_id to the verified auth_peer_id. The datagram's own buffer is
        // reused when nothing else holds it; a shared buffer is copied.
        let mut rewritten = match datagram.try_into_mut() {
            Ok(buffer) => buffer,
            Err(shared) => BytesMut::from(&shared[..]),
        };
        if MediaHeader::overwrite_sender_peer_id(&mut rewritten, auth_peer_id).is_err() {
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let datagram_bytes = rewritten.freeze();

        // 3. Routing according to media type
        let room = match self.sessions.voice_rooms.get(&header.channel_id) {
            Some(r) => r,
            None => return,
        };

        match header.media_type {
            MediaType::Audio => {
                // Broadcast to all participants in this voice channel except sender and deafened users
                for &target_peer_id in &room.participants {
                    if target_peer_id == auth_peer_id {
                        continue;
                    }
                    if let Some(target_session) = self.sessions.sessions.get(&target_peer_id)
                        && !target_session.deafened
                    {
                        forward_datagram(&target_session.connection, datagram_bytes.clone());
                    }
                }
            }
            MediaType::VideoDelta | MediaType::ScreenAudio => {
                // Send only to active viewers of this streamer
                if let Some(viewers) = room.screen_subscribers.get(&auth_peer_id) {
                    for &viewer_peer_id in viewers {
                        if let Some(viewer_session) = self.sessions.sessions.get(&viewer_peer_id) {
                            forward_datagram(&viewer_session.connection, datagram_bytes.clone());
                        }
                    }
                }
            }
            MediaType::VideoKey => {
                // VideoKey is handled via Uni-Streams (route_uni_keyframe)
            }
        }
    }

    /// Forward an IDR keyframe received on a Uni-Stream to all subscribed viewers. The keyframe is
    /// charged to the sender's media budget like any datagram: it is larger, so it weighs more.
    pub async fn route_uni_keyframe(
        &self,
        channel_id: u32,
        auth_peer_id: u32,
        header: &MediaHeader,
        payload: Bytes,
    ) {
        let budget = match self.sessions.sessions.get(&auth_peer_id) {
            Some(s) => s.media.clone(),
            None => {
                TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        if !admit(&budget, MEDIA_HEADER_SIZE + payload.len()) {
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let room = match self.sessions.voice_rooms.get(&channel_id) {
            Some(r) => r,
            None => return,
        };

        if let Some(viewers) = room.screen_subscribers.get(&auth_peer_id) {
            let mut stamped_header = *header;
            stamped_header.sender_peer_id = auth_peer_id;
            let header_bytes = stamped_header.to_bytes();

            for &viewer_peer_id in viewers {
                if let Some(viewer_session) = self.sessions.sessions.get(&viewer_peer_id) {
                    let conn = viewer_session.connection.clone();
                    let h_bytes = header_bytes;
                    let p_bytes = payload.clone();

                    tokio::spawn(async move {
                        let len = (h_bytes.len() + p_bytes.len()) as u64;
                        let sent = async {
                            let mut send_stream = conn.open_uni().await.ok()?;
                            send_stream.write_all(&h_bytes).await.ok()?;
                            send_stream.write_all(&p_bytes).await.ok()?;
                            send_stream.finish().ok()
                        }
                        .await
                        .is_some();
                        if sent {
                            TRAFFIC.bytes_out.fetch_add(len, Ordering::Relaxed);
                        } else {
                            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
            }
        }
    }
}
