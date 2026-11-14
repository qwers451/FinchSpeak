//! Selective Forwarding Unit (SFU) router.
//!
//! Validates incoming datagrams and uni-streams, checks room membership,
//! rewrites `sender_peer_id` to prevent spoofing, and fans out media to subscribers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use finch_protocol::media::{MediaHeader, MediaType, MEDIA_HEADER_SIZE};

use crate::health::TRAFFIC;
use crate::limits::MediaRateLimiter;
use crate::session::SessionManager;

/// A session whose datagrams go to the wrong voice channel is logged at most once per this interval.
const WRONG_CHANNEL_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Rate-limits a log line that one session can produce for every datagram it sends. It lives on the
/// session (`ClientSession::wrong_channel_warn`), so it is dropped together with the session: there is
/// no table of peers to prune.
#[derive(Debug)]
pub struct WarnThrottle {
    origin: Instant,
    /// Time of the last logged line in milliseconds after `origin`, plus one; 0 when nothing was logged.
    last_logged_plus_one: AtomicU64,
}

impl Default for WarnThrottle {
    fn default() -> Self {
        Self { origin: Instant::now(), last_logged_plus_one: AtomicU64::new(0) }
    }
}

impl WarnThrottle {
    /// `true` when a line may be logged now (none logged yet, or `interval` has passed since the last
    /// one). A `true` answer starts the next interval. Lock-free: safe on the datagram path.
    pub fn allow(&self, now: Instant, interval: Duration) -> bool {
        let now_ms = u64::try_from(now.saturating_duration_since(self.origin).as_millis()).unwrap_or(u64::MAX);
        let interval_ms = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
        let mut stored = self.last_logged_plus_one.load(Ordering::Relaxed);
        loop {
            if stored != 0 && now_ms < (stored - 1).saturating_add(interval_ms) {
                return false;
            }
            match self.last_logged_plus_one.compare_exchange_weak(
                stored,
                now_ms.saturating_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => stored = actual,
            }
        }
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
}

impl SfuRouter {
    pub fn new(sessions: SessionManager) -> Self {
        Self { sessions }
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
            let may_log = self
                .sessions
                .sessions
                .get(&auth_peer_id)
                .is_some_and(|s| s.wrong_channel_warn.allow(Instant::now(), WRONG_CHANNEL_LOG_INTERVAL));
            if may_log {
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

    /// Whether a key frame stream from `auth_peer_id` on `channel_id` is worth reading at all:
    /// the sender has a started share and at least one viewer. Checked before the body is read.
    pub fn keyframe_wanted(&self, channel_id: u32, auth_peer_id: u32) -> bool {
        let sharing = self.sessions.sessions.get(&auth_peer_id).is_some_and(|s| s.sharing_screen);
        if !sharing || !may_send(MediaType::VideoKey, sharing, false) {
            return false;
        }
        self.sessions
            .voice_rooms
            .get(&channel_id)
            .is_some_and(|room| room.screen_subscribers.get(&auth_peer_id).is_some_and(|viewers| !viewers.is_empty()))
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
    fn wrong_channel_warning_is_logged_once_per_interval() {
        let throttle = WarnThrottle::default();
        let start = throttle.origin;
        let interval = WRONG_CHANNEL_LOG_INTERVAL;
        assert!(throttle.allow(start, interval), "the first drop is logged");
        assert!(!throttle.allow(start + Duration::from_secs(1), interval), "a flood is not logged again");
        assert!(!throttle.allow(start + Duration::from_millis(9_999), interval), "still inside the interval");
        assert!(throttle.allow(start + Duration::from_secs(10), interval), "logged again after the interval");
        assert!(!throttle.allow(start + Duration::from_secs(11), interval), "and throttled again");
    }

    #[test]
    fn each_session_has_its_own_throttle() {
        let first = WarnThrottle::default();
        let second = WarnThrottle::default();
        let now = first.origin + Duration::from_secs(1);
        assert!(first.allow(now, WRONG_CHANNEL_LOG_INTERVAL));
        assert!(second.allow(now, WRONG_CHANNEL_LOG_INTERVAL), "another session is logged on its own");
    }
}
