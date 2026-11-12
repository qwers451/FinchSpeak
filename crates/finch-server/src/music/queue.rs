//! Authoritative room timeline and ordered control delivery.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use finch_protocol::messages::{MusicCommand, ServerMessage};
use finch_protocol::models::{MusicRoomState, MusicTrack, Playing, QueueEntry};
use uuid::Uuid;

use super::{MusicService, TEMP_LEASE_SECS};
use crate::error::ServerError;
use crate::permissions::{self, Permission};
use crate::session::SessionManager;

#[derive(Default)]
pub struct RoomMusic {
    pub state: MusicRoomState,
    revision: u64,
    timer: Option<tokio::task::AbortHandle>,
}

impl RoomMusic {
    pub fn clear(&mut self) -> Vec<MusicTrack> {
        self.cancel_timer();
        let mut removed: Vec<_> = self
            .state
            .queue
            .drain(..)
            .map(|entry| entry.track)
            .collect();
        if let Some(playing) = self.state.playing.take() {
            removed.push(playing.entry.track);
        }
        self.revision = self.revision.wrapping_add(1);
        removed
    }

    fn cancel_timer(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
    }

    fn advance(&mut self, now: u64) -> Vec<MusicTrack> {
        let removed = self
            .state
            .playing
            .take()
            .map(|playing| playing.entry.track)
            .into_iter()
            .collect();
        self.state.playing = if self.state.queue.is_empty() {
            None
        } else {
            Some(Playing {
                entry: self.state.queue.remove(0),
                position_ms: 0,
                at_server_ms: now,
                paused: false,
            })
        };
        removed
    }

    fn apply(
        &mut self,
        command: MusicCommand,
        actor: Uuid,
        staff: bool,
        track: Option<MusicTrack>,
        max_queue: usize,
        now: u64,
    ) -> Result<Vec<MusicTrack>, ServerError> {
        let removed = match command {
            MusicCommand::Enqueue { .. } => {
                if self.state.queue.len() + usize::from(self.state.playing.is_some()) >= max_queue {
                    return Err(ServerError::BadRequest("Music queue is full".into()));
                }
                let track =
                    track.ok_or_else(|| ServerError::NotFound("Music track not found".into()))?;
                let entry = QueueEntry {
                    entry_id: Uuid::new_v4(),
                    track,
                    added_by: actor,
                };
                if self.state.playing.is_none() {
                    self.state.playing = Some(Playing {
                        entry,
                        position_ms: 0,
                        at_server_ms: now,
                        paused: false,
                    });
                } else {
                    self.state.queue.push(entry);
                }
                Vec::new()
            }
            MusicCommand::Pause => {
                let playing = self.playing_mut()?;
                playing.position_ms = position(playing, now);
                playing.at_server_ms = now;
                playing.paused = true;
                Vec::new()
            }
            MusicCommand::Resume => {
                let playing = self.playing_mut()?;
                if playing.paused {
                    playing.at_server_ms = now;
                    playing.paused = false;
                }
                Vec::new()
            }
            MusicCommand::Seek { position_ms } => {
                let playing = self.playing_mut()?;
                playing.position_ms = position_ms.min(playing.entry.track.duration_ms);
                playing.at_server_ms = now;
                Vec::new()
            }
            MusicCommand::Skip => self.advance(now),
            MusicCommand::Remove { entry_id } => {
                if let Some(playing) = self
                    .state
                    .playing
                    .as_ref()
                    .filter(|playing| playing.entry.entry_id == entry_id)
                {
                    if !staff && playing.entry.added_by != actor {
                        return Err(ServerError::Forbidden);
                    }
                    self.advance(now)
                } else {
                    let index = self
                        .state
                        .queue
                        .iter()
                        .position(|entry| entry.entry_id == entry_id)
                        .ok_or_else(|| {
                            ServerError::NotFound("Music queue entry not found".into())
                        })?;
                    if !staff && self.state.queue[index].added_by != actor {
                        return Err(ServerError::Forbidden);
                    }
                    vec![self.state.queue.remove(index).track]
                }
            }
            MusicCommand::Clear => {
                if !staff {
                    return Err(ServerError::Forbidden);
                }
                self.clear()
            }
        };
        self.cancel_timer();
        self.revision = self.revision.wrapping_add(1);
        Ok(removed)
    }

    fn playing_mut(&mut self) -> Result<&mut Playing, ServerError> {
        self.state
            .playing
            .as_mut()
            .ok_or_else(|| ServerError::BadRequest("Nothing is playing".into()))
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn position(playing: &Playing, now: u64) -> u64 {
    let elapsed = if playing.paused {
        0
    } else {
        now.saturating_sub(playing.at_server_ms)
    };
    playing
        .position_ms
        .saturating_add(elapsed)
        .min(playing.entry.track.duration_ms)
}

impl MusicService {
    pub async fn command(
        self: &Arc<Self>,
        sessions: &SessionManager,
        peer: u32,
        command: MusicCommand,
    ) -> Result<(), ServerError> {
        self.require_enabled()?;
        self.command_hits
            .hit(
                &peer.to_string(),
                Instant::now(),
                20,
                Duration::from_secs(10),
            )
            .map_err(ServerError::TooManyRequests)?;
        let _catalogue = self.catalogue.lock().await;
        let (user, short) = {
            let session = sessions
                .sessions
                .get(&peer)
                .ok_or(ServerError::Unauthorized)?;
            if session.server_muted {
                return Err(ServerError::Forbidden);
            }
            (
                session.user_id,
                session.current_voice_channel.ok_or_else(|| {
                    ServerError::BadRequest("Join a voice room to control music".into())
                })?,
            )
        };
        let track = if let MusicCommand::Enqueue { track_id } = &command {
            let track = self.record(*track_id).await?.track()?;
            if track.temporary {
                if track.uploader_id != user {
                    return Err(ServerError::Forbidden);
                }
                let claimed = sqlx::query("UPDATE music_tracks SET queued_in = ? WHERE id = ? AND temporary = 1 AND queued_in IS NULL AND deleting = 0 AND created_at > ?")
                    .bind(short as i64).bind(track_id.to_string()).bind(crate::api::invites::now_secs() - TEMP_LEASE_SECS)
                    .execute(&self.pool).await?.rows_affected();
                if claimed != 1 {
                    return Err(ServerError::BadRequest(
                        "Temporary music track is already queued or expired".into(),
                    ));
                }
            }
            Some(track)
        } else {
            None
        };
        let temporary_claim = track
            .as_ref()
            .filter(|track| track.temporary)
            .map(|track| track.id);
        // A participant may have left or been muted while the DB lookup was in progress.
        // Keep the session read guard only through the synchronous room mutation.
        let applied = (|| {
            let session = sessions
                .sessions
                .get(&peer)
                .ok_or(ServerError::Unauthorized)?;
            if session.server_muted {
                return Err(ServerError::Forbidden);
            }
            if session.current_voice_channel != Some(short) {
                return Err(ServerError::BadRequest("Your voice room changed".into()));
            }
            let staff = permissions::allows(&session.role, Permission::ModerateVoice);
            let mut room = sessions
                .voice_rooms
                .get_mut(&short)
                .ok_or_else(|| ServerError::NotFound("Voice room not found".into()))?;
            if !room.participants.contains(&peer) {
                return Err(ServerError::Forbidden);
            }
            room.music
                .apply(command, user, staff, track, self.config.max_queue, now_ms())
        })();
        let removed = match applied {
            Ok(removed) => removed,
            Err(error) => {
                if let Some(id) = temporary_claim {
                    sqlx::query(
                        "UPDATE music_tracks SET queued_in = NULL WHERE id = ? AND queued_in = ?",
                    )
                    .bind(id.to_string())
                    .bind(short as i64)
                    .execute(&self.pool)
                    .await?;
                }
                return Err(error);
            }
        };
        self.publish(sessions, short);
        self.arm_timer(sessions.clone(), short);
        self.dispose_tracks_locked(removed).await;
        Ok(())
    }

    pub fn send_state(&self, sessions: &SessionManager, peer: u32) {
        if !self.config.enabled {
            return;
        }
        let short = sessions
            .sessions
            .get(&peer)
            .and_then(|session| session.current_voice_channel);
        let Some(short) = short else { return };
        if let Some(message) = self.message(sessions, short) {
            sessions.send_control_to_peer(peer, &message);
        }
    }

    fn message(&self, sessions: &SessionManager, short: u32) -> Option<ServerMessage> {
        let channel_id = sessions
            .short_to_channel
            .get(&short)
            .map(|channel| *channel)?;
        let state = sessions
            .voice_rooms
            .get(&short)
            .map(|room| room.music.state.clone())?;
        Some(ServerMessage::MusicState {
            channel_id,
            state,
            server_ms: now_ms(),
        })
    }

    fn publish(&self, sessions: &SessionManager, short: u32) {
        let Some(message) = self.message(sessions, short) else {
            return;
        };
        let peers: Vec<_> = sessions
            .voice_rooms
            .get(&short)
            .map(|room| room.participants.iter().copied().collect())
            .unwrap_or_default();
        for peer in peers {
            sessions.send_control_to_peer(peer, &message);
        }
    }

    fn arm_timer(self: &Arc<Self>, sessions: SessionManager, short: u32) {
        let timer = sessions.voice_rooms.get(&short).and_then(|room| {
            let playing = room
                .music
                .state
                .playing
                .as_ref()
                .filter(|playing| !playing.paused)?;
            Some((
                room.music.revision,
                playing.entry.entry_id,
                Duration::from_millis(
                    playing
                        .entry
                        .track
                        .duration_ms
                        .saturating_sub(position(playing, now_ms())),
                ),
            ))
        });
        let Some((revision, entry, delay)) = timer else {
            return;
        };
        let service = self.clone();
        let timer_sessions = sessions.clone();
        let timer = tokio::spawn(async move {
            let sessions = timer_sessions;
            tokio::time::sleep(delay).await;
            let _catalogue = service.catalogue.lock().await;
            let removed =
                {
                    let Some(mut room) = sessions.voice_rooms.get_mut(&short) else {
                        return;
                    };
                    if room.music.revision != revision
                        || !room.music.state.playing.as_ref().is_some_and(|playing| {
                            playing.entry.entry_id == entry && !playing.paused
                        })
                    {
                        return;
                    }
                    room.music.timer.take();
                    let removed = room.music.advance(now_ms());
                    room.music.revision = room.music.revision.wrapping_add(1);
                    removed
                };
            service.publish(&sessions, short);
            service.arm_timer(sessions.clone(), short);
            service.dispose_tracks_locked(removed).await;
        });
        if let Some(mut room) = sessions.voice_rooms.get_mut(&short) {
            if room.music.revision == revision {
                room.music.cancel_timer();
                room.music.timer = Some(timer.abort_handle());
            } else {
                timer.abort();
            }
        } else {
            timer.abort();
        }
    }

    async fn dispose_tracks_locked(&self, tracks: Vec<MusicTrack>) {
        for track in tracks.into_iter().filter(|track| track.temporary) {
            if let Err(error) = self.delete_file(&track).await {
                tracing::warn!("Temporary music removal will retry {}: {error}", track.id);
            }
        }
    }

    /// Called after synchronous membership changes have emptied or deleted a room.
    pub fn dispose_tracks(self: &Arc<Self>, tracks: Vec<MusicTrack>) {
        if tracks.is_empty() {
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            let _catalogue = service.catalogue.lock().await;
            service.dispose_tracks_locked(tracks).await;
        });
    }

    /// Evict a deleted library track from every queue. Caller owns catalogue.
    pub fn evict_track(self: &Arc<Self>, sessions: &SessionManager, id: Uuid) {
        let mut changed = Vec::new();
        for mut room in sessions.voice_rooms.iter_mut() {
            let before = room.music.state.queue.len();
            room.music.state.queue.retain(|entry| entry.track.id != id);
            let active = room
                .music
                .state
                .playing
                .as_ref()
                .is_some_and(|playing| playing.entry.track.id == id);
            if active {
                room.music.advance(now_ms());
            }
            if active || before != room.music.state.queue.len() {
                room.music.cancel_timer();
                room.music.revision = room.music.revision.wrapping_add(1);
                changed.push(*room.key());
            }
        }
        for short in changed {
            self.publish(sessions, short);
            self.arm_timer(sessions.clone(), short);
        }
    }

    pub fn can_download_temporary(
        &self,
        sessions: &SessionManager,
        user: Uuid,
        track: &MusicTrack,
        short: Option<i64>,
    ) -> bool {
        if track.uploader_id == user {
            return true;
        }
        let Some(short) = short.and_then(|short| u32::try_from(short).ok()) else {
            return false;
        };
        let peers: Vec<_> = sessions
            .sessions
            .iter()
            .filter(|session| {
                session.user_id == user
                    && session.client_level >= finch_protocol::CLIENT_LEVEL_1_5
                    && session.current_voice_channel == Some(short)
            })
            .map(|session| session.peer_id)
            .collect();
        sessions.voice_rooms.get(&short).is_some_and(|room| {
            peers.iter().any(|peer| room.participants.contains(peer))
                && (room
                    .music
                    .state
                    .playing
                    .as_ref()
                    .is_some_and(|playing| playing.entry.track.id == track.id)
                    || room
                        .music
                        .state
                        .queue
                        .iter()
                        .any(|entry| entry.track.id == track.id))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_preserves_pause_seek_resume_and_queue_order() {
        let actor = Uuid::new_v4();
        let make_track = || MusicTrack {
            id: Uuid::new_v4(),
            uploader_id: actor,
            title: "Track".into(),
            artist: None,
            duration_ms: 10_000,
            size_bytes: 4,
            format: "mp3".into(),
            temporary: false,
            created_at: 0,
        };
        let first = make_track();
        let second = make_track();
        let mut room = RoomMusic::default();
        room.apply(
            MusicCommand::Enqueue { track_id: first.id },
            actor,
            false,
            Some(first.clone()),
            100,
            1_000,
        )
        .unwrap();
        room.apply(
            MusicCommand::Enqueue {
                track_id: second.id,
            },
            actor,
            false,
            Some(second.clone()),
            100,
            1_500,
        )
        .unwrap();
        room.apply(MusicCommand::Pause, actor, false, None, 100, 3_000)
            .unwrap();
        assert_eq!(
            position(room.state.playing.as_ref().unwrap(), 50_000),
            2_000
        );
        room.apply(
            MusicCommand::Seek { position_ms: 4_000 },
            actor,
            false,
            None,
            100,
            60_000,
        )
        .unwrap();
        room.apply(MusicCommand::Resume, actor, false, None, 100, 80_000)
            .unwrap();
        assert_eq!(
            position(room.state.playing.as_ref().unwrap(), 80_500),
            4_500
        );
        assert_eq!(room.advance(81_000), vec![first]);
        assert_eq!(room.state.playing.unwrap().entry.track, second);
    }
}
