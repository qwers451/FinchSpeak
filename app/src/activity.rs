//! Windows game discovery and session-bound sharing. Only short titles reach the control stream.
#[cfg(any(windows, test))]
mod classification;
#[cfg(any(windows, test))]
mod tracking;
#[cfg(windows)]
mod windows;

use finch_protocol::ClientMessage;
use finch_protocol::models::{ServerState, UserStatus};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::AppState;

#[cfg(windows)]
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Default)]
pub struct ActivityRuntime {
    generation: u64,
    peer_id: u32,
    user_id: Option<Uuid>,
    supported: bool,
    enabled: bool,
    dnd: bool,
    revision: u64,
    ui_revision: u64,
    tx: Option<mpsc::Sender<ClientMessage>>,
    reported: Option<String>,
}

impl ActivityRuntime {
    pub fn connect(
        &mut self,
        generation: u64,
        peer_id: u32,
        user_id: Uuid,
        snapshot: &ServerState,
        tx: mpsc::Sender<ClientMessage>,
    ) {
        self.revision += 1;
        self.generation = generation;
        self.peer_id = peer_id;
        self.user_id = Some(user_id);
        self.supported = cfg!(windows)
            && snapshot
                .features
                .iter()
                .any(|feature| feature == "activity");
        self.enabled = false;
        // The first connection may receive its own presence only after authentication.
        self.dnd = snapshot
            .online_users
            .iter()
            .find(|user| user.id == user_id)
            .is_none_or(|user| user.status == UserStatus::DoNotDisturb);
        self.tx = Some(tx);
        self.reported = None;
    }

    pub fn disconnect(&mut self) {
        self.revision += 1;
        self.enabled = false;
        self.peer_id = 0;
        self.tx = None;
        self.reported = None;
    }

    #[cfg(any(windows, test))]
    fn poll_ticket(&self) -> Option<u64> {
        (self.supported && self.enabled && !self.dnd && self.tx.is_some()).then_some(self.revision)
    }

    async fn clear(&mut self) {
        if self.supported
            && let Some(tx) = &self.tx
        {
            let _ = tx.send(ClientMessage::SetActivity { game: None }).await;
        }
        self.reported = None;
    }

    async fn set_sharing(&mut self, generation: u64, peer_id: u32, enabled: bool, revision: u64) {
        if generation != self.generation || peer_id != self.peer_id || revision <= self.ui_revision
        {
            return;
        }
        self.ui_revision = revision;
        self.revision += 1;
        self.enabled = enabled;
        if !enabled {
            self.clear().await;
        }
    }

    pub async fn set_status(&mut self, user_id: Uuid, status: UserStatus) {
        if self.user_id != Some(user_id) {
            return;
        }
        let dnd = status == UserStatus::DoNotDisturb;
        if self.dnd != dnd {
            self.dnd = dnd;
            self.revision += 1;
            if dnd {
                self.clear().await;
            }
        }
    }

    #[cfg(any(windows, test))]
    async fn publish(&mut self, ticket: u64, game: Option<String>) {
        if self.poll_ticket() != Some(ticket) || game == self.reported {
            return;
        }
        if let Some(tx) = &self.tx
            && tx
                .send(ClientMessage::SetActivity { game: game.clone() })
                .await
                .is_ok()
        {
            self.reported = game;
        }
    }
}

#[tauri::command]
pub async fn set_activity_sharing(
    session_generation: u64,
    peer_id: u32,
    enabled: bool,
    revision: u64,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .game_activity
        .lock()
        .await
        .set_sharing(session_generation, peer_id, enabled, revision)
        .await;
    Ok(())
}

#[cfg(windows)]
pub fn spawn_polling(state: AppState, generation: u64, peer_id: u32) {
    tokio::spawn(async move {
        let mut tracker = tracking::Tracker::default();
        let mut interval = tokio::time::interval(POLL_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let ticket = {
                let runtime = state.game_activity.lock().await;
                if runtime.generation != generation || runtime.peer_id != peer_id {
                    return;
                }
                runtime.poll_ticket()
            };
            let Some(ticket) = ticket else {
                tracker = tracking::Tracker::default();
                continue;
            };
            // File metadata can touch disk. Privacy changes remain responsive while the probe runs.
            let result = tokio::task::spawn_blocking(move || {
                let game = tracker.poll(&windows::WindowsProbe);
                (tracker, game)
            })
            .await;
            let Ok((next_tracker, game)) = result else {
                return;
            };
            tracker = next_tracker;
            state.game_activity.lock().await.publish(ticket, game).await;
        }
    });
}

#[cfg(not(windows))]
pub fn spawn_polling(_: AppState, _: u64, _: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected() -> (ActivityRuntime, mpsc::Receiver<ClientMessage>, Uuid) {
        let (tx, rx) = mpsc::channel(16);
        let user = Uuid::new_v4();
        let runtime = ActivityRuntime {
            generation: 7,
            peer_id: 42,
            user_id: Some(user),
            supported: true,
            enabled: true,
            dnd: false,
            tx: Some(tx),
            ..ActivityRuntime::default()
        };
        (runtime, rx, user)
    }

    #[tokio::test]
    async fn activity_opt_out_clears_immediately_and_rejects_a_queued_poll() {
        let (mut runtime, mut rx, _) = connected();
        let ticket = runtime.poll_ticket().unwrap();
        runtime.set_sharing(7, 42, false, 2).await;
        assert_eq!(
            rx.recv().await,
            Some(ClientMessage::SetActivity { game: None })
        );
        runtime.publish(ticket, Some("Portal".into())).await;
        runtime.set_sharing(7, 42, true, 1).await;
        assert!(runtime.poll_ticket().is_none());
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn activity_dnd_and_disconnect_reject_stale_results() {
        let (mut runtime, mut rx, user) = connected();
        let ticket = runtime.poll_ticket().unwrap();
        runtime.set_status(user, UserStatus::DoNotDisturb).await;
        assert_eq!(
            rx.recv().await,
            Some(ClientMessage::SetActivity { game: None })
        );
        runtime.publish(ticket, Some("Portal".into())).await;
        assert!(rx.try_recv().is_err());
        runtime.set_status(user, UserStatus::Online).await;
        let ticket = runtime.poll_ticket().unwrap();
        runtime.disconnect();
        runtime.publish(ticket, Some("Portal".into())).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn activity_requires_feature_and_an_exact_session_identity() {
        let (mut runtime, mut rx, _) = connected();
        runtime.supported = false;
        assert!(runtime.poll_ticket().is_none());
        runtime.publish(0, Some("Portal".into())).await;
        assert!(rx.try_recv().is_err());
        runtime.supported = true;
        runtime.enabled = false;
        runtime.set_sharing(6, 42, true, 1).await;
        runtime.set_sharing(7, 41, true, 2).await;
        assert!(runtime.poll_ticket().is_none());
        runtime.set_sharing(7, 42, true, 3).await;
        assert!(runtime.poll_ticket().is_some());
    }

    #[tokio::test]
    async fn activity_connect_waits_for_own_status_and_requires_a_fresh_opt_in() {
        let (mut runtime, _, user) = connected();
        let (tx, mut rx) = mpsc::channel(8);
        let mut snapshot = ServerState {
            channels: vec![],
            online_users: vec![],
            voice_states: vec![],
            unread: vec![],
            features: vec![],
            activities: vec![],
        };
        runtime.connect(8, 43, user, &snapshot, tx.clone());
        runtime.set_sharing(8, 43, true, 1).await;
        runtime.set_status(user, UserStatus::Online).await;
        assert!(
            runtime.poll_ticket().is_none(),
            "legacy server must not be polled"
        );
        snapshot.features.push("activity".into());
        runtime.connect(9, 44, user, &snapshot, tx);
        assert!(
            runtime.poll_ticket().is_none(),
            "connect must forget the old preference command"
        );
        runtime.set_sharing(9, 44, true, 2).await;
        assert!(
            runtime.poll_ticket().is_none(),
            "unknown own status must suppress activity"
        );
        runtime.set_status(user, UserStatus::Online).await;
        assert_eq!(runtime.poll_ticket().is_some(), cfg!(windows));
        assert!(rx.try_recv().is_err());
    }
}
