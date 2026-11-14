//! Selective Forwarding Unit (SFU) router.
//!
//! Validates incoming datagrams and uni-streams, checks room membership,
//! rewrites `sender_peer_id` to prevent spoofing, and fans out media to subscribers.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};

use crate::health::TRAFFIC;
use crate::limits::MediaRateLimiter;
use crate::session::SessionManager;

/// A peer whose datagrams go to the wrong voice channel is logged at most once per this interval.
const WRONG_CHANNEL_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Once this many peers are tracked, peers whose interval has passed are dropped from the table.
const PRUNE_AT: usize = 1024;

/// Rate-limits log lines that a single peer can produce for every datagram it sends.
#[derive(Default)]
pub struct LogThrottle {
    last_logged: Mutex<HashMap<u32, Instant>>,
}

impl LogThrottle {
    /// `true` when this peer may log now (never logged, or the interval has passed). A `true`
    /// answer starts the peer's interval.
    pub fn allow(&self, peer_id: u32, now: Instant) -> bool {
        let mut last_logged = self.last_logged.lock().unwrap_or_else(PoisonError::into_inner);
        if last_logged
            .get(&peer_id)
            .is_some_and(|logged| now.saturating_duration_since(*logged) < WRONG_CHANNEL_LOG_INTERVAL)
        {
            return false;
        }
        if last_logged.len() >= PRUNE_AT {
            last_logged.retain(|_, logged| now.saturating_duration_since(*logged) < WRONG_CHANNEL_LOG_INTERVAL);
        }
        last_logged.insert(peer_id, now);
        true
    }
}

/// Hand one datagram to a receiver and count it: bytes out, or a drop when it could not be sent.
fn forward_datagram(connection: &quinn::Connection, datagram: Bytes) {
    let len = datagram.len() as u64;
    if connection.send_datagram(datagram).is_ok() {
        TRAFFIC.bytes_out.fetch_add(len, Ordering::Relaxed);
    } else {
        TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
    }
}

/// Charge a keyframe to a participant's keyframe bucket; `false` when it is over the limit.
fn admit_keyframe(budget: &Mutex<MediaRateLimiter>, bytes: usize) -> bool {
    budget
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .allow_keyframe(Instant::now(), bytes)
}

/// Charge `bytes` to a participant's media budget; `false` when it is over the limit.
fn admit(budget: &Mutex<MediaRateLimiter>, bytes: usize) -> bool {
    budget
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .allow(Instant::now(), bytes)
}

/// Whether a sender in this state may send a datagram of `media_type`. Screen video needs a started
/// share; screen sound needs one too, and a moderator's server mute silences it. Voice is governed
/// by the mute check in `route_datagram`.
fn may_send(media_type: MediaType, sharing: bool, server_muted: bool) -> bool {
    match media_type {
        MediaType::Audio => true,
        MediaType::ScreenAudio => sharing && !server_muted,
        MediaType::VideoDelta | MediaType::VideoKey => sharing,
    }
}

/// SFU Media Router.
#[derive(Clone)]
pub struct SfuRouter {
    sessions: SessionManager,
    wrong_channel_log: Arc<LogThrottle>,
}

impl SfuRouter {
    pub fn new(sessions: SessionManager) -> Self {
        Self { sessions, wrong_channel_log: Arc::new(LogThrottle::default()) }
    }

    /// Route an incoming media datagram. Taken by value so the sender-id rewrite can reuse its buffer.
    pub async fn route_datagram(&self, datagram: Bytes, auth_peer_id: u32) {
        TRAFFIC.bytes_in.fetch_add(datagram.len() as u64, Ordering::Relaxed);

        // Everything the sender's state is needed for is copied out of its session here, so the
        // hot path takes no extra lock.
        let (voice_channel, sending_muted, server_muted, sharing, budget) = match self.sessions.sessions.get(&auth_peer_id) {
            Some(s) => (s.current_voice_channel, s.muted || s.server_muted, s.server_muted, s.sharing_screen, s.media.clone()),
            None => {
                TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // A datagram the sender may not send at all is refused before it is charged: it spends no budget.
        let header = MediaHeader::from_slice(&datagram).ok();
        if header.as_ref().is_some_and(|h| !may_send(h.media_type, sharing, server_muted)) {
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // The participant's media budget (bytes and datagrams per second) is charged for every
        // datagram it sends that may be sent, malformed ones included, before anything is routed.
        if !admit(&budget, datagram.len()) {
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let Some(header) = header else {
            // Too short, or an unknown media type: there is nothing to route.
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };

        // 1. Validate that the sender is in the room specified in the header
        if voice_channel != Some(header.channel_id) {
            if self.wrong_channel_log.allow(auth_peer_id, Instant::now()) {
                tracing::warn!(
                    "Peer {auth_peer_id} attempted to send to channel {} but is in {:?}",
                    header.channel_id,
                    voice_channel
                );
            }
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // A muted microphone sends no voice; the sound of a shared screen is a separate stream.
        if sending_muted && header.media_type == MediaType::Audio {
            return;
        }

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
    /// charged to the sender's keyframe bucket, and only when somebody is watching: nothing sent,
    /// nothing charged. Datagram budgets are not touched.
    pub async fn route_uni_keyframe(
        &self,
        channel_id: u32,
        auth_peer_id: u32,
        header: &MediaHeader,
        payload: Bytes,
    ) {
        let (budget, sharing) = match self.sessions.sessions.get(&auth_peer_id) {
            Some(s) => (s.media.clone(), s.sharing_screen),
            None => {
                TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        // Keyframes are screen video: only a sender with a started share may send them.
        if !may_send(MediaType::VideoKey, sharing, false) {
            TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let room = match self.sessions.voice_rooms.get(&channel_id) {
            Some(r) => r,
            None => return,
        };

        if let Some(viewers) = room.screen_subscribers.get(&auth_peer_id).filter(|viewers| !viewers.is_empty()) {
            if !admit_keyframe(&budget, MEDIA_HEADER_SIZE + payload.len()) {
                TRAFFIC.datagrams_dropped.fetch_add(1, Ordering::Relaxed);
                return;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_channel_warning_is_logged_once_per_peer_per_interval() {
        let throttle = LogThrottle::default();
        let start = Instant::now();
        assert!(throttle.allow(7, start), "the first drop is logged");
        assert!(!throttle.allow(7, start + Duration::from_secs(1)), "a flood is not logged again");
        assert!(!throttle.allow(7, start + Duration::from_secs(9)), "still inside the interval");
        assert!(throttle.allow(8, start + Duration::from_secs(1)), "another peer is logged on its own");
        assert!(throttle.allow(7, start + Duration::from_secs(10)), "logged again after the interval");
    }

    #[test]
    fn throttle_table_stays_bounded_when_many_peers_flood_once() {
        let throttle = LogThrottle::default();
        let start = Instant::now();
        for peer in 0..(PRUNE_AT as u32 * 3) {
            assert!(throttle.allow(peer, start));
        }
        let later = start + WRONG_CHANNEL_LOG_INTERVAL;
        assert!(throttle.allow(0, later));
        let tracked = throttle.last_logged.lock().unwrap().len();
        assert!(tracked <= PRUNE_AT + 1, "tracked peers: {tracked}");
    }
}
