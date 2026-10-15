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
    pub token_id: String,
    pub peer_id: u32,
    pub user_id: Uuid,
    pub username: String,
    pub role: String,
    pub current_voice_channel: Option<u32>,
    pub muted: bool,
    pub deafened: bool,
    /// True while this peer is sharing its screen in its voice channel.
    pub sharing_screen: bool,
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
            if let dashmap::mapref::entry::Entry::Occupied(mut entry) = self.user_to_peer.entry(session.user_id)
                && *entry.get() == peer_id
            {
                let remaining = self.sessions.iter()
                    .filter(|active| active.user_id == session.user_id)
                    .map(|active| active.peer_id)
                    .max();
                if let Some(remaining) = remaining {
                    entry.insert(remaining);
                } else {
                    entry.remove();
                }
            }
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
    /// Mark whether `peer_id` is sharing its screen. Returns the previous value.
    pub fn set_sharing(&self, peer_id: u32, sharing: bool) -> bool {
        self.sessions
            .get_mut(&peer_id)
            .map(|mut session| std::mem::replace(&mut session.sharing_screen, sharing))
            .unwrap_or(false)
    }

    pub fn leave_voice(&self, peer_id: u32, short_id: u32) {
        if let Some(mut session) = self.sessions.get_mut(&peer_id)
            && session.current_voice_channel == Some(short_id)
        {
            session.current_voice_channel = None;
        }

        self.remove_peer_from_voice_room(peer_id, short_id);
    }

    /// Subscribe viewer to streamer's screen share.
    /// Subscribe a viewer to a streamer's screen; only members of the same voice room may watch.
    pub fn subscribe_screen(&self, viewer_peer_id: u32, streamer_peer_id: u32) {
        let short_id = self.sessions.get(&streamer_peer_id).and_then(|s| s.current_voice_channel);
        let viewer_room = self.sessions.get(&viewer_peer_id).and_then(|s| s.current_voice_channel);
        if short_id.is_none() || viewer_room != short_id {
            tracing::warn!(
                "Peer {viewer_peer_id} tried to watch peer {streamer_peer_id} outside their voice room"
            );
            return;
        }
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
        let targets: Vec<_> = self
            .sessions
            .iter()
            .map(|entry| (*entry.key(), entry.value().control_tx.clone(), entry.value().connection.clone()))
            .collect();
        deliver(targets, &msg);
    }

    pub async fn broadcast_voice(&self, short_id: u32, msg: ServerMessage, exclude_peer_id: Option<u32>) {
        let targets: Vec<_> = if let Some(room) = self.voice_rooms.get(&short_id) {
            room.participants
                .iter()
                .filter(|&&peer_id| Some(peer_id) != exclude_peer_id)
                .filter_map(|&peer_id| {
                    self.sessions
                        .get(&peer_id)
                        .map(|s| (peer_id, s.control_tx.clone(), s.connection.clone()))
                })
                .collect()
        } else {
            Vec::new()
        };
        deliver(targets, &msg);
    }
}

/// Queue `msg` for every target without waiting. A client whose control queue is full is not
/// keeping up; it is disconnected instead of stalling delivery to everyone else.
fn deliver(targets: Vec<(u32, Sender<ServerMessage>, quinn::Connection)>, msg: &ServerMessage) {
    for (peer_id, tx, connection) in targets {
        if let Err(tokio::sync::mpsc::error::TrySendError::Full(_)) = tx.try_send(msg.clone()) {
            tracing::warn!("Control queue of peer {peer_id} is full; disconnecting the slow client");
            connection.close(0u32.into(), b"control queue overflow");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn connection_pair() -> (quinn::Endpoint, quinn::Endpoint, quinn::Connection, quinn::Connection) {
        let (cert, key) = crate::quic::generate_self_signed_cert().unwrap();
        let config = crate::quic::make_server_config_with(cert.clone(), key).unwrap();
        let server = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let mut tls = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        tls.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];
        let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap();
        let mut client = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client.set_default_client_config(quinn::ClientConfig::new(Arc::new(crypto)));
        let outgoing = client.connect(server.local_addr().unwrap(), "localhost").unwrap();
        let incoming = async { server.accept().await.unwrap().await.unwrap() };
        let (client_conn, server_conn) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(outgoing, incoming)
        }).await.expect("local QUIC handshake timed out");
        (server, client, server_conn, client_conn.unwrap())
    }

    async fn registered_twice() -> (SessionManager, Uuid, quinn::Endpoint, quinn::Endpoint, quinn::Connection) {
        let (server, client, connection, remote) = connection_pair().await;
        let manager = SessionManager::new();
        let user_id = Uuid::new_v4();
        for peer_id in [1001, 1002] {
            let (control_tx, _) = tokio::sync::mpsc::channel(8);
            manager.register_session(ClientSession {
                token_id: String::new(),
                peer_id, user_id, username: "alice".into(), role: "member".into(),
                current_voice_channel: None, muted: false, deafened: false, sharing_screen: false,
                connection: connection.clone(), control_tx,
            });
        }
        (manager, user_id, server, client, remote)
    }

    #[tokio::test]
    async fn aud030_older_disconnect_preserves_newer_mapping() {
        let (manager, user, _server, _client, _remote) = registered_twice().await;
        manager.unregister_session(1001);
        assert_eq!(manager.user_to_peer.get(&user).map(|peer| *peer), Some(1002));
    }

    #[tokio::test]
    async fn aud030_newer_disconnect_restores_remaining_session() {
        let (manager, user, _server, _client, _remote) = registered_twice().await;
        manager.unregister_session(1002);
        assert_eq!(manager.user_to_peer.get(&user).map(|peer| *peer), Some(1001));
        manager.unregister_session(1001);
        assert!(!manager.user_to_peer.contains_key(&user));
    }

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
