//! Pure clock arithmetic shared by the music transport and its tests.

use finch_protocol::models::Playing;

/// Position the room should be at, on the server clock.
pub fn target_position(playing: &Playing, server_now_ms: u64) -> u64 {
    if playing.paused {
        playing.position_ms
    } else {
        playing.position_ms.saturating_add(server_now_ms.saturating_sub(playing.at_server_ms))
    }
}

/// A local position further than 250 ms from the target is restarted; closer is left alone.
pub fn needs_resync(current_ms: u64, target_ms: u64) -> bool {
    current_ms.abs_diff(target_ms) > 250
}

#[cfg(test)]
mod tests {
    use super::{needs_resync, target_position};
    use finch_protocol::models::{MusicTrack, Playing, QueueEntry};
    use uuid::Uuid;

    fn playing(position_ms: u64, at_server_ms: u64, paused: bool) -> Playing {
        Playing {
            entry: QueueEntry {
                entry_id: Uuid::new_v4(),
                added_by: Uuid::new_v4(),
                track: MusicTrack {
                    id: Uuid::new_v4(),
                    uploader_id: Uuid::new_v4(),
                    title: "Track".into(),
                    artist: None,
                    duration_ms: 60_000,
                    size_bytes: 4096,
                    format: "wav".into(),
                    temporary: false,
                    created_at: 0,
                },
            },
            position_ms,
            at_server_ms,
            paused,
        }
    }

    #[test]
    fn playing_target_follows_the_server_clock() {
        assert_eq!(target_position(&playing(500, 1000, false), 1350), 850);
    }

    #[test]
    fn paused_target_stays_at_the_saved_position() {
        assert_eq!(target_position(&playing(500, 1000, true), 9_000), 500);
    }

    #[test]
    fn a_server_clock_before_the_start_mark_does_not_underflow() {
        assert_eq!(target_position(&playing(10, 3000, false), 1000), 10);
    }

    #[test]
    fn resync_happens_only_beyond_250_ms() {
        assert!(!needs_resync(1000, 1250));
        assert!(!needs_resync(1250, 1000));
        assert!(needs_resync(1000, 1251));
        assert!(needs_resync(1251, 1000));
    }
}
