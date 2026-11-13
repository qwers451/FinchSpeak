use finch_protocol::messages::{MusicCommand, ServerMessage};
use finch_protocol::models::MusicTrack;
use finch_server::config::ServerConfig;
use finch_server::music::MusicService;
use finch_server::session::{ClientSession, SessionManager};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(3);

struct Room {
    sessions: SessionManager,
    service: Arc<MusicService>,
    receivers: Vec<mpsc::Receiver<ServerMessage>>,
    _dir: tempfile::TempDir,
    _server: quinn::Endpoint,
    _client: quinn::Endpoint,
    _remote: quinn::Connection,
}

impl Room {
    async fn new(max_queue: usize) -> anyhow::Result<Self> {
        let (cert, key) = finch_server::quic::generate_self_signed_cert()?;
        let server_config = finch_server::quic::make_server_config_with(cert.clone(), key)?;
        let server = quinn::Endpoint::server(server_config, "127.0.0.1:0".parse()?)?;
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert)?;
        let mut tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        tls.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];
        let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)?;
        let mut client = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
        client.set_default_client_config(quinn::ClientConfig::new(Arc::new(crypto)));
        let outgoing = client.connect(server.local_addr()?, "localhost")?;
        let incoming = async { server.accept().await.unwrap().await.unwrap() };
        let (remote, local) =
            tokio::time::timeout(WAIT, async { tokio::join!(outgoing, incoming) }).await?;
        let dir = tempfile::tempdir()?;
        let pool = finch_server::db::init_db("sqlite::memory:").await?;
        let mut config = ServerConfig::default();
        config.music.storage_dir = Some(dir.path().to_string_lossy().into());
        config.music.max_queue = max_queue;
        let service = Arc::new(MusicService::new(pool, &config));
        service.startup_cleanup().await?;
        let sessions = SessionManager::new();
        sessions.music.set(service.clone()).ok();
        sessions.register_channel(Uuid::new_v4(), 2);
        sessions.register_channel(Uuid::new_v4(), 3);
        let mut receivers = Vec::new();
        for peer in 1..=4 {
            let (tx, rx) = mpsc::channel(128);
            receivers.push(rx);
            sessions.register_session(ClientSession {
                token_id: format!("token{peer}"),
                peer_id: peer,
                user_id: Uuid::new_v4(),
                username: format!("User {peer}"),
                role: if peer == 1 { "admin" } else { "member" }.into(),
                current_voice_channel: None,
                muted: false,
                deafened: false,
                sharing_screen: false,
                server_muted: false,
                supports_voice_extensions: true,
                client_level: if peer == 4 { 0 } else { 150 },
                activity: None,
                last_activity_update: None,
                connection: local.clone(),
                control_tx: tx,
                media: sessions.media_budget(),
            });
        }
        Ok(Self {
            sessions,
            service,
            receivers,
            _dir: dir,
            _server: server,
            _client: client,
            _remote: remote?,
        })
    }

    fn user(&self, peer: u32) -> Uuid {
        self.sessions.sessions.get(&peer).unwrap().user_id
    }

    async fn track(&self, peer: u32, temporary: bool, duration: u64) -> anyhow::Result<MusicTrack> {
        let track = MusicTrack {
            id: Uuid::new_v4(),
            uploader_id: self.user(peer),
            title: "Track".into(),
            artist: None,
            duration_ms: duration,
            size_bytes: 4,
            format: "mp3".into(),
            temporary,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64,
        };
        let path = self.service.file_path(track.id, "mp3")?;
        tokio::fs::write(&path, b"ID3x").await?;
        sqlx::query("INSERT INTO music_tracks (id, uploader_id, title, duration_ms, size_bytes, format, path, temporary, created_at) VALUES (?, ?, 'Track', ?, 4, 'mp3', ?, ?, ?)")
            .bind(track.id.to_string()).bind(track.uploader_id.to_string()).bind(duration as i64)
            .bind(path.to_string_lossy().as_ref()).bind(temporary).bind(track.created_at)
            .execute(&self.service.pool).await?;
        Ok(track)
    }

    async fn command(
        &self,
        peer: u32,
        command: MusicCommand,
    ) -> Result<(), finch_server::error::ServerError> {
        self.service.command(&self.sessions, peer, command).await
    }

    fn state(&self) -> finch_protocol::models::MusicRoomState {
        self.sessions
            .voice_rooms
            .get(&2)
            .unwrap()
            .music
            .state
            .clone()
    }
}

#[tokio::test]
async fn queue_obeys_membership_mute_permissions_limits_and_legacy_filtering() -> anyhow::Result<()>
{
    let mut room = Room::new(2).await?;
    let track = room.track(2, false, 60_000).await?;
    assert!(
        room.command(2, MusicCommand::Enqueue { track_id: track.id })
            .await
            .is_err()
    );
    for peer in 1..=4 {
        room.sessions.join_voice(peer, 2);
    }
    room.command(2, MusicCommand::Enqueue { track_id: track.id })
        .await?;
    assert_eq!(room.state().playing.as_ref().unwrap().entry.track, track);
    assert!(matches!(
        room.receivers[1].recv().await,
        Some(ServerMessage::MusicState { .. })
    ));
    assert!(
        room.receivers[3].try_recv().is_err(),
        "legacy clients must not receive new variants"
    );
    room.command(3, MusicCommand::Enqueue { track_id: track.id })
        .await?;
    assert!(
        room.command(2, MusicCommand::Enqueue { track_id: track.id })
            .await
            .is_err()
    );
    let entry = room.state().queue[0].entry_id;
    assert!(
        room.command(2, MusicCommand::Remove { entry_id: entry })
            .await
            .is_err()
    );
    room.command(3, MusicCommand::Remove { entry_id: entry })
        .await?;
    assert!(room.command(2, MusicCommand::Clear).await.is_err());
    room.sessions.sessions.get_mut(&2).unwrap().server_muted = true;
    assert!(room.command(2, MusicCommand::Pause).await.is_err());
    room.command(1, MusicCommand::Clear).await?;
    assert!(room.state().playing.is_none() && room.state().queue.is_empty());
    Ok(())
}

#[tokio::test]
async fn temporary_queue_lease_is_unique_and_uploader_exit_does_not_interrupt_playback()
-> anyhow::Result<()> {
    let room = Room::new(100).await?;
    for peer in 1..=3 {
        room.sessions.join_voice(peer, 2);
    }
    let track = room.track(2, true, 60_000).await?;
    room.command(2, MusicCommand::Enqueue { track_id: track.id })
        .await?;
    room.sessions.leave_voice(2, 2);
    assert_eq!(room.state().playing.unwrap().entry.track.id, track.id);
    assert!(room.service.file_path(track.id, "mp3")?.exists());
    room.sessions.join_voice(2, 3);
    assert!(
        room.command(2, MusicCommand::Enqueue { track_id: track.id })
            .await
            .is_err()
    );
    room.command(3, MusicCommand::Skip).await?;
    assert!(room.service.record(track.id).await.is_err());
    assert!(!room.service.file_path(track.id, "mp3")?.exists());
    Ok(())
}

#[tokio::test]
async fn automatic_advance_ignores_old_seek_timer_and_empty_room_cleans_temporaries()
-> anyhow::Result<()> {
    let mut room = Room::new(100).await?;
    room.sessions.join_voice(2, 2);
    let first = room.track(2, true, 1_000).await?;
    let second = room.track(2, true, 60_000).await?;
    room.command(2, MusicCommand::Enqueue { track_id: first.id })
        .await?;
    room.command(
        2,
        MusicCommand::Enqueue {
            track_id: second.id,
        },
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(550)).await;
    room.command(2, MusicCommand::Seek { position_ms: 0 })
        .await?;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        room.state().playing.as_ref().unwrap().entry.track.id,
        first.id
    );
    tokio::time::timeout(WAIT, async {
        loop {
            let Some(message) = room.receivers[1].recv().await else { panic!("control queue closed") };
            if matches!(message, ServerMessage::MusicState { state, .. } if state.playing.as_ref().is_some_and(|playing| playing.entry.track.id == second.id)) { break; }
        }
    }).await?;
    let first_path = room.service.file_path(first.id, "mp3")?;
    tokio::time::timeout(WAIT, async {
        while first_path.exists() || room.service.record(first.id).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    room.sessions.leave_voice(2, 2);
    assert!(room.state().playing.is_none() && room.state().queue.is_empty());
    tokio::time::timeout(WAIT, async {
        while room.service.file_path(second.id, "mp3").unwrap().exists()
            || room.service.record(second.id).await.is_ok()
        {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(room.service.record(second.id).await.is_err());
    Ok(())
}

#[tokio::test]
async fn pause_resume_keeps_position_and_commands_are_limited_per_session() -> anyhow::Result<()> {
    let room = Room::new(100).await?;
    room.sessions.join_voice(2, 2);
    let track = room.track(2, false, 60_000).await?;
    room.command(2, MusicCommand::Enqueue { track_id: track.id })
        .await?;
    room.command(2, MusicCommand::Seek { position_ms: 1_234 })
        .await?;
    room.command(2, MusicCommand::Pause).await?;
    let position = room.state().playing.unwrap().position_ms;
    tokio::time::sleep(Duration::from_millis(80)).await;
    room.command(2, MusicCommand::Resume).await?;
    assert_eq!(room.state().playing.unwrap().position_ms, position);
    for _ in 0..16 {
        room.command(2, MusicCommand::Pause).await?;
    }
    assert!(matches!(
        room.command(2, MusicCommand::Resume).await,
        Err(finch_server::error::ServerError::TooManyRequests(_))
    ));
    Ok(())
}
