//! Session management, peer IDs allocation, and active voice room registry.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use finch_protocol::messages::ServerMessage;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

/// A connected client session.
#[derive(Clone)]
pub struct ClientSession {
    pub peer_id: u32,
    pub user_id: Uuid,
    pub username: String,
    pub role: String,
    pub current_voice_channel: Option<u32>,
    pub muted: bool,
    pub deafened: bool,
    pub connection: quinn::Connection,
    pub control_tx: Sender<ServerMessage>,
}

/// A voice room tracking active participants.
#[derive(Default)]
pub struct VoiceRoom {
    /// Peer IDs of users currently in this voice room.
    pub participants: HashSet<u32>,
    /// Streamers in this room and their active viewers (streamer_peer_id -> set of viewer_peer_ids).
    pub screen_subscribers: HashMapStreamers,
}

type HashMapStreamers = std::collections::HashMap<u32, HashSet<u32>>;

/// Registry managing active sessions and rooms.
#[derive(Clone)]
pub struct SessionManager {
    next_peer_id: Arc<AtomicU32>,
    /// Map session_peer_id -> ClientSession
    pub sessions: Arc<DashMap<u32, ClientSession>>,
    /// Map user UUID -> session_peer_id
    pub user_to_peer: Arc<DashMap<Uuid, u32>>,
    /// Map channel short_id -> VoiceRoom
    pub voice_rooms: Arc<DashMap<u32, VoiceRoom>>,
    /// Map channel UUID -> short_id
    pub channel_to_short: Arc<DashMap<Uuid, u32>>,
    /// Map channel short_id -> channel UUID
    pub short_to_channel: Arc<DashMap<u32, Uuid>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            next_peer_id: Arc::new(AtomicU32::new(1001)),
            sessions: Arc::new(DashMap::new()),
            user_to_peer: Arc::new(DashMap::new()),
            voice_rooms: Arc::new(DashMap::new()),
            channel_to_short: Arc::new(DashMap::new()),
            short_to_channel: Arc::new(DashMap::new()),
        }
    }

    /// Allocate a new monotonic session peer ID.
    pub fn allocate_peer_id(&self) -> u32 {
        self.next_peer_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Register a channel mapping.
    pub fn register_channel(&self, channel_id: Uuid, short_id: u32) {
        self.channel_to_short.insert(channel_id, short_id);
        self.short_to_channel.insert(short_id, channel_id);
        self.voice_rooms.entry(short_id).or_default();
    }

    /// Register an authenticated user session.
    pub fn register_session(&self, session: ClientSession) {
        self.user_to_peer.insert(session.user_id, session.peer_id);
        self.sessions.insert(session.peer_id, session);
    }

    /// Unregister a session upon disconnection.
    pub fn unregister_session(&self, peer_id: u32) {
        if let Some((_, session)) = self.sessions.remove(&peer_id) {
            self.user_to_peer.remove(&session.user_id);
            if let Some(short_id) = session.current_voice_channel {
                self.leave_voice(peer_id, short_id);
            }
        }
    }

    /// Remove peer from voice room state without touching sessions map.
    fn remove_peer_from_voice_room(&self, peer_id: u32, short_id: u32) {
        if let Some(mut room) = self.voice_rooms.get_mut(&short_id) {
            room.participants.remove(&peer_id);
            room.screen_subscribers.remove(&peer_id);
            for viewers in room.screen_subscribers.values_mut() {
                viewers.remove(&peer_id);
            }
        }
    }

    /// Join a voice room by short_id.
    pub fn join_voice(&self, peer_id: u32, short_id: u32) {
        let old_short = if let Some(mut session) = self.sessions.get_mut(&peer_id) {
            let old = session.current_voice_channel;
            session.current_voice_channel = Some(short_id);
            old
        } else {
            None
        };

        if let Some(old) = old_short {
            self.remove_peer_from_voice_room(peer_id, old);
        }

        let mut room = self.voice_rooms.entry(short_id).or_default();
        room.participants.insert(peer_id);
    }

    /// Leave a voice room.
    pub fn leave_voice(&self, peer_id: u32, short_id: u32) {
        if let Some(mut session) = self.sessions.get_mut(&peer_id)
            && session.current_voice_channel == Some(short_id)
        {
            session.current_voice_channel = None;
        }

        self.remove_peer_from_voice_room(peer_id, short_id);
    }

    /// Subscribe viewer to streamer's screen share.
    pub fn subscribe_screen(&self, viewer_peer_id: u32, streamer_peer_id: u32) {
        let short_id = self.sessions.get(&streamer_peer_id).and_then(|s| s.current_voice_channel);
        if let Some(short_id) = short_id
            && let Some(mut room) = self.voice_rooms.get_mut(&short_id)
        {
            room.screen_subscribers
                .entry(streamer_peer_id)
                .or_default()
                .insert(viewer_peer_id);
        }
    }

    /// Unsubscribe viewer from streamer's screen share.
    pub fn unsubscribe_screen(&self, viewer_peer_id: u32, streamer_peer_id: u32) {
        let short_id = self.sessions.get(&streamer_peer_id).and_then(|s| s.current_voice_channel);
        if let Some(short_id) = short_id
            && let Some(mut room) = self.voice_rooms.get_mut(&short_id)
            && let Some(viewers) = room.screen_subscribers.get_mut(&streamer_peer_id)
        {
            viewers.remove(&viewer_peer_id);
        }
    }

    /// Broadcast a Control Stream message to all connected sessions.
    pub async fn broadcast_all(&self, msg: ServerMessage) {
        let senders: Vec<_> = self.sessions.iter().map(|entry| entry.value().control_tx.clone()).collect();
        for tx in senders {
            let _ = tx.send(msg.clone()).await;
        }
    }

    /// Broadcast a Control Stream message to participants of a voice room.
    pub async fn broadcast_voice(&self, short_id: u32, msg: ServerMessage, exclude_peer_id: Option<u32>) {
        let senders: Vec<_> = if let Some(room) = self.voice_rooms.get(&short_id) {
            room.participants
                .iter()
                .filter(|&&peer_id| Some(peer_id) != exclude_peer_id)
                .filter_map(|&peer_id| self.sessions.get(&peer_id).map(|s| s.control_tx.clone()))
                .collect()
        } else {
            Vec::new()
        };

        for tx in senders {
            let _ = tx.send(msg.clone()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_id_allocation() {
        let manager = SessionManager::new();
        assert_eq!(manager.allocate_peer_id(), 1001);
        assert_eq!(manager.allocate_peer_id(), 1002);
    }

    #[test]
    fn test_channel_mapping() {
        let manager = SessionManager::new();
        let uuid = Uuid::new_v4();
        manager.register_channel(uuid, 5);

        assert_eq!(manager.channel_to_short.get(&uuid).map(|v| *v), Some(5));
        assert_eq!(manager.short_to_channel.get(&5).map(|v| *v), Some(uuid));
    }
}
