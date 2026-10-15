//! Selective Forwarding Unit (SFU) router.
//!
//! Validates incoming datagrams and uni-streams, checks room membership,
//! rewrites `sender_peer_id` to prevent spoofing, and fans out media to subscribers.

use bytes::{Bytes, BytesMut};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};

use crate::session::SessionManager;

/// SFU Media Router.
#[derive(Clone)]
pub struct SfuRouter {
    sessions: SessionManager,
}

impl SfuRouter {
    pub fn new(sessions: SessionManager) -> Self {
        Self { sessions }
    }

    /// Route an incoming media datagram.
    pub async fn route_datagram(&self, raw_datagram: &[u8], auth_peer_id: u32) {
        if raw_datagram.len() < MEDIA_HEADER_SIZE {
            return;
        }

        let header = match MediaHeader::from_slice(raw_datagram) {
            Ok(h) => h,
            Err(_) => return,
        };

        // 1. Validate that the sender is in the room specified in the header
        let session = match self.sessions.sessions.get(&auth_peer_id) {
            Some(s) => s.clone(),
            None => return,
        };

        if session.current_voice_channel != Some(header.channel_id) {
            tracing::warn!(
                "Peer {auth_peer_id} attempted to send to channel {} but is in {:?}",
                header.channel_id,
                session.current_voice_channel
            );
            return;
        }

        // If user is muted and sending audio, drop
        if session.muted && (header.media_type == MediaType::Audio || header.media_type == MediaType::ScreenAudio) {
            return;
        }

        // 2. Rewrite sender_peer_id to the verified auth_peer_id
        let mut rewritten = BytesMut::from(raw_datagram);
        if MediaHeader::overwrite_sender_peer_id(&mut rewritten, auth_peer_id).is_err() {
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
                        let _ = target_session.connection.send_datagram(datagram_bytes.clone());
                    }
                }
            }
            MediaType::VideoDelta | MediaType::ScreenAudio => {
                // Send only to active viewers of this streamer
                if let Some(viewers) = room.screen_subscribers.get(&auth_peer_id) {
                    for &viewer_peer_id in viewers {
                        if let Some(viewer_session) = self.sessions.sessions.get(&viewer_peer_id) {
                            let _ = viewer_session.connection.send_datagram(datagram_bytes.clone());
                        }
                    }
                }
            }
            MediaType::VideoKey => {
                // VideoKey is handled via Uni-Streams (route_uni_keyframe)
            }
        }
    }

    /// Forward an IDR keyframe received on a Uni-Stream to all subscribed viewers.
    pub async fn route_uni_keyframe(
        &self,
        channel_id: u32,
        auth_peer_id: u32,
        header: &MediaHeader,
        payload: Bytes,
    ) {
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
                        if let Ok(mut send_stream) = conn.open_uni().await
                            && send_stream.write_all(&h_bytes).await.is_ok()
                            && send_stream.write_all(&p_bytes).await.is_ok()
                        {
                            let _ = send_stream.finish();
                        }
                    });
                }
            }
        }
    }
}
