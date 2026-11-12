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
    /// The name shown to other users (display name, or the login when none is set).
    pub username: String,
    pub role: String,
    pub current_voice_channel: Option<u32>,
    pub muted: bool,
    pub deafened: bool,
    /// True while this peer is sharing its screen in its voice channel.
    pub sharing_screen: bool,
    /// Silenced by a moderator; cleared when the person leaves the room.
    pub server_muted: bool,
    /// Accepts `VoiceMoved` and `JoinSoundChanged` on the control stream.
    pub supports_voice_extensions: bool,
    pub client_level: u16,
    pub activity: Option<String>,
    pub last_activity_update: Option<std::time::Instant>,
    pub connection: quinn::Connection,
    pub control_tx: Sender<ServerMessage>,
}

/// A voice room tracking active participants.
#[derive(Default)]
pub struct VoiceRoom {
    pub music: crate::music::queue::RoomMusic,
    /// Peer IDs of users currently in this voice room.
    pub participants: HashSet<u32>,
    /// Streamers in this room and their active viewers (streamer_peer_id -> set of viewer_peer_ids).
    pub screen_subscribers: HashMapStreamers,
}

type HashMapStreamers = std::collections::HashMap<u32, HashSet<u32>>;

/// Registry managing active sessions and rooms.
#[derive(Clone)]
pub struct SessionManager {
    /// Configured before the API/QUIC listeners begin accepting clients.
    pub music: Arc<std::sync::OnceLock<Arc<crate::music::MusicService>>>,
    /// Soundboard settings, set with the router; unset means the default (enabled).
    pub soundboard: Arc<std::sync::OnceLock<crate::config::SoundboardConfig>>,
    /// One active reporting device per user. Serializes updates, clears and snapshots.
    activities: Arc<std::sync::Mutex<std::collections::HashMap<Uuid, (u32, String)>>>,
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
    /// When each user last had a typing hint forwarded (to throttle them).
    pub last_typing: Arc<DashMap<Uuid, std::time::Instant>>,
    /// Messages per person, to stop floods: `(limit, window)`.
    message_rule: Arc<std::sync::Mutex<(usize, std::time::Duration)>>,
    message_hits: Arc<crate::limits::WindowLimiter>,
    /// Other things a person could repeat endlessly (status changes).
    action_hits: Arc<crate::limits::WindowLimiter>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    /// Keep optional control variants away from clients that cannot decode them.
    fn accepts_message(session: &ClientSession, msg: &ServerMessage) -> bool {
        session.client_level >= msg.minimum_client_level()
            && (!matches!(msg, ServerMessage::JoinSoundChanged { .. } | ServerMessage::VoiceMoved { .. })
                || session.supports_voice_extensions)
    }

    pub fn send_control_to_peer(&self, peer_id: u32, msg: &ServerMessage) {
        let targets = self.sessions.get(&peer_id)
            .filter(|session| Self::accepts_message(session, msg))
            .map(|session| vec![(peer_id, session.control_tx.clone(), session.connection.clone())])
            .unwrap_or_default();
        deliver(targets, msg);
    }
    pub fn new() -> Self {
        Self {
            music: Arc::new(std::sync::OnceLock::new()),
            soundboard: Arc::new(std::sync::OnceLock::new()),
            activities: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            next_peer_id: Arc::new(AtomicU32::new(1001)),
            sessions: Arc::new(DashMap::new()),
            user_to_peer: Arc::new(DashMap::new()),
            voice_rooms: Arc::new(DashMap::new()),
            channel_to_short: Arc::new(DashMap::new()),
            short_to_channel: Arc::new(DashMap::new()),
            last_typing: Arc::new(DashMap::new()),
            message_rule: Arc::new(std::sync::Mutex::new((30, std::time::Duration::from_secs(10)))),
            message_hits: Arc::new(crate::limits::WindowLimiter::default()),
            action_hits: Arc::new(crate::limits::WindowLimiter::default()),
        }
    }

    /// Set how many messages one person may send per window.
    pub fn set_message_rule(&self, limit: usize, window: std::time::Duration) {
        if let Ok(mut rule) = self.message_rule.lock() {
            *rule = (limit.max(1), window);
        }
    }

    /// Count one `kind` of action of `user`: at most `max` per `window`; `Err(seconds)` when over.
    pub fn allow_action(&self, user: Uuid, kind: &str, max: usize, window: std::time::Duration) -> Result<(), u64> {
        self.action_hits.hit(&format!("{kind}:{user}"), std::time::Instant::now(), max, window)
    }

    /// Count a message of `user`; `Err(seconds)` when they are sending too fast.
    pub fn allow_message(&self, user: Uuid) -> Result<(), u64> {
        let (limit, window) = self.message_rule.lock().map(|rule| *rule).unwrap_or((30, std::time::Duration::from_secs(10)));
        self.message_hits.hit(&user.to_string(), std::time::Instant::now(), limit, window)
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
        let mut activities = self.activities.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((_, session)) = self.sessions.remove(&peer_id) {
            if activities.get(&session.user_id).is_some_and(|(owner, _)| *owner == peer_id) {
                activities.remove(&session.user_id);
                self.broadcast_control(&ServerMessage::ActivityChanged { user_id: session.user_id, game: None });
            }
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

    pub fn activities(&self) -> Vec<finch_protocol::models::UserActivity> {
        self.activities.lock().unwrap_or_else(|error| error.into_inner()).iter()
            .map(|(&user_id, (_, game))| finch_protocol::models::UserActivity { user_id, game: game.clone() })
            .collect()
    }

    /// Clears always bypass the nonempty update cooldown so privacy changes take effect at once.
    pub fn set_activity(&self, peer_id: u32, game: Option<&str>, now: std::time::Instant) {
        let game = game.map(|title| title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(64).collect::<String>())
            .filter(|title| !title.is_empty());
        let mut activities = self.activities.lock().unwrap_or_else(|error| error.into_inner());
        let Some(mut session) = self.sessions.get_mut(&peer_id) else { return; };
        if game.is_some() {
            if session.last_activity_update.is_some_and(|last| now.saturating_duration_since(last) < std::time::Duration::from_secs(5)) {
                return;
            }
            session.last_activity_update = Some(now);
        }
        session.activity = game.clone();
        let user_id = session.user_id;
        drop(session);
        let previous = activities.get(&user_id).map(|(_, game)| game.clone());
        match &game {
            Some(game) => { activities.insert(user_id, (peer_id, game.clone())); }
            None if activities.get(&user_id).is_some_and(|(owner, _)| *owner == peer_id) => { activities.remove(&user_id); }
            None => return,
        }
        if previous != game {
            self.broadcast_control(&ServerMessage::ActivityChanged { user_id, game });
        }
    }

    /// Remove peer from voice room state without touching sessions map.
    fn remove_peer_from_voice_room(&self, peer_id: u32, short_id: u32) {
        let removed = if let Some(mut room) = self.voice_rooms.get_mut(&short_id) {
            room.participants.remove(&peer_id);
            room.screen_subscribers.remove(&peer_id);
            for viewers in room.screen_subscribers.values_mut() {
                viewers.remove(&peer_id);
            }
            if room.participants.is_empty() { room.music.clear() } else { Vec::new() }
        } else { Vec::new() };
        if let Some(music) = self.music.get() { music.dispose_tracks(removed); }
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

        if let Some(old) = old_short.filter(|old| *old != short_id) {
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
            session.server_muted = false;
        }

        self.remove_peer_from_voice_room(peer_id, short_id);
    }

    /// Take everyone out of the voice room of `short_id` and forget the room.
    /// Returns who was in it: `(peer_id, user_id, was_sharing_screen)`.
    pub fn close_voice_room(&self, short_id: u32) -> Vec<(u32, Uuid, bool)> {
        let participants: Vec<u32> = self
            .voice_rooms
            .get(&short_id)
            .map(|room| room.participants.iter().copied().collect())
            .unwrap_or_default();
        let mut evicted = Vec::new();
        for peer_id in participants {
            let info = self.sessions.get_mut(&peer_id).map(|mut session| {
                session.current_voice_channel = None;
                (session.user_id, std::mem::replace(&mut session.sharing_screen, false))
            });
            if let Some((user_id, was_sharing)) = info {
                evicted.push((peer_id, user_id, was_sharing));
            }
        }
        if let Some((_, mut room)) = self.voice_rooms.remove(&short_id)
            && let Some(music) = self.music.get()
        { music.dispose_tracks(room.music.clear()); }
        if let Some((_, channel_id)) = self.short_to_channel.remove(&short_id) {
            self.channel_to_short.remove(&channel_id);
        }
        evicted
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
        self.broadcast_control(&msg);
    }

    fn broadcast_control(&self, msg: &ServerMessage) {
        let targets: Vec<_> = self
            .sessions
            .iter()
            .filter(|session| Self::accepts_message(session, msg))
            .map(|entry| (*entry.key(), entry.value().control_tx.clone(), entry.value().connection.clone()))
            .collect();
        deliver(targets, msg);
    }

    /// Broadcast to everybody except the sessions of `user_id`.
    pub async fn broadcast_except_user(&self, msg: ServerMessage, user_id: Uuid) {
        let targets: Vec<_> = self
            .sessions
            .iter()
            .filter(|entry| entry.value().user_id != user_id)
            .filter(|session| Self::accepts_message(session, &msg))
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
                        .filter(|session| Self::accepts_message(session, &msg))
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

    #[tokio::test]
    async fn negotiated_control_variants_are_filtered_on_every_delivery_path() {
        let (server, client, connection, remote) = connection_pair().await;
        let manager = SessionManager::new();
        let room = 5;
        manager.register_channel(Uuid::new_v4(), room);
        let mut receivers = Vec::new();
        for (peer_id, client_level) in [(1001, 0), (1002, 149), (1003, 150)] {
            let (control_tx, control_rx) = tokio::sync::mpsc::channel(8);
            manager.register_session(ClientSession {
                token_id: String::new(), peer_id, user_id: Uuid::new_v4(),
                username: "client".into(), role: "member".into(), current_voice_channel: None,
                muted: false, deafened: false, sharing_screen: false, server_muted: false,
                supports_voice_extensions: true, client_level, activity: None, last_activity_update: None,
                connection: connection.clone(), control_tx,
            });
            manager.join_voice(peer_id, room);
            receivers.push((client_level, control_rx));
        }
        let message = ServerMessage::ActivityChanged { user_id: Uuid::new_v4(), game: Some("Game".into()) };
        manager.broadcast_all(message.clone()).await;
        manager.broadcast_except_user(message.clone(), Uuid::new_v4()).await;
        manager.broadcast_voice(room, message.clone(), None).await;
        for peer in [1001, 1002, 1003] { manager.send_control_to_peer(peer, &message); }
        for (level, mut receiver) in receivers {
            if level >= 150 {
                for _ in 0..4 { assert_eq!(receiver.try_recv().unwrap(), message); }
            }
            assert!(receiver.try_recv().is_err(), "unsupported control variant reached level {level}");
        }
        drop((server, client, remote));
    }

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
                current_voice_channel: None, muted: false, deafened: false, sharing_screen: false, server_muted: false,
                supports_voice_extensions: false, client_level: 0, activity: None, last_activity_update: None,
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
    async fn activity_cooldown_is_per_session_and_clear_does_not_reopen_it() {
        let (manager, user, _server, _client, _remote) = registered_twice().await;
        let now = std::time::Instant::now();
        manager.set_activity(1001, Some("Portal"), now);
        assert_eq!(manager.activities()[0].game, "Portal");
        manager.set_activity(1001, Some("Hades"), now + std::time::Duration::from_secs(4));
        assert_eq!(manager.activities()[0].game, "Portal");
        manager.set_activity(1001, None, now + std::time::Duration::from_secs(4));
        assert!(manager.activities().is_empty());
        manager.set_activity(1001, Some("Hades"), now + std::time::Duration::from_secs(4));
        assert!(manager.activities().is_empty());
        manager.set_activity(1001, Some("Hades"), now + std::time::Duration::from_secs(5));
        assert_eq!(manager.activities()[0].game, "Hades");
        manager.set_activity(1002, Some("Other device"), now);
        assert_eq!(manager.activities()[0].game, "Other device");
        manager.unregister_session(1001);
        assert_eq!(manager.activities()[0].game, "Other device");
        manager.unregister_session(1002);
        assert!(!manager.activities().iter().any(|activity| activity.user_id == user));
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
