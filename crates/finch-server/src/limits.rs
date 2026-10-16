//! Rate limits: wrong passwords, registrations, and message floods.

use std::collections::VecDeque;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// The address a REST request came from (added by the HTTPS listener).
#[derive(Debug, Clone, Copy)]
pub struct PeerAddr(pub IpAddr);

/// Counts wrong passwords per key and locks the key for a while after too many.
#[derive(Default)]
pub struct LoginGuard {
    entries: DashMap<String, Entry>,
}

#[derive(Default, Clone, Copy)]
struct Entry {
    failures: u32,
    locked_until: Option<Instant>,
}

impl LoginGuard {
    /// `Err(seconds to wait)` while `key` is locked.
    pub fn check(&self, key: &str, now: Instant) -> Result<(), u64> {
        match self.entries.get(key).and_then(|entry| entry.locked_until) {
            Some(until) if until > now => Err((until - now).as_secs() + 1),
            _ => Ok(()),
        }
    }

    /// A wrong password. Returns `Err(seconds)` if this one locked the key.
    pub fn failure(&self, key: &str, now: Instant, max_failures: u32, lockout: Duration) -> Result<(), u64> {
        let mut entry = self.entries.entry(key.to_string()).or_default();
        // A lock that has run out starts counting again from zero.
        if entry.locked_until.is_some_and(|until| until <= now) {
            *entry = Entry::default();
        }
        entry.failures += 1;
        if entry.failures >= max_failures {
            entry.locked_until = Some(now + lockout);
            entry.failures = 0;
            return Err(lockout.as_secs().max(1));
        }
        Ok(())
    }

    pub fn success(&self, key: &str) {
        self.entries.remove(key);
        if self.entries.len() > 10_000 {
            let now = Instant::now();
            self.entries.retain(|_, entry| entry.locked_until.is_some_and(|until| until > now));
        }
    }
}

/// At most `max` events per `window` per key.
#[derive(Default)]
pub struct WindowLimiter {
    events: DashMap<String, VecDeque<Instant>>,
}

impl WindowLimiter {
    /// Count one event; `Err(seconds to wait)` when the key is over its limit (the event is not counted then).
    pub fn hit(&self, key: &str, now: Instant, max: usize, window: Duration) -> Result<(), u64> {
        let mut queue = self.events.entry(key.to_string()).or_default();
        while queue.front().is_some_and(|first| now.duration_since(*first) >= window) {
            queue.pop_front();
        }
        if queue.len() >= max {
            let wait = queue.front().map_or(window, |first| window.saturating_sub(now.duration_since(*first)));
            return Err(wait.as_secs() + 1);
        }
        queue.push_back(now);
        drop(queue);
        if self.events.len() > 10_000 {
            self.events.retain(|_, queue| queue.back().is_some_and(|last| now.duration_since(*last) < window));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_wrong_passwords_lock_the_key_for_the_lockout_and_a_success_resets() {
        let guard = LoginGuard::default();
        let t0 = Instant::now();
        let lock = Duration::from_secs(60);
        for _ in 0..4 {
            assert!(guard.failure("alice", t0, 5, lock).is_ok());
        }
        guard.success("alice");
        for _ in 0..4 {
            assert!(guard.failure("alice", t0, 5, lock).is_ok(), "the success started a new count");
        }
        assert_eq!(guard.failure("alice", t0, 5, lock), Err(60));
        assert!(guard.check("alice", t0 + Duration::from_secs(10)).is_err());
        assert!(guard.check("bob", t0).is_ok(), "other keys are untouched");
        assert!(guard.check("alice", t0 + Duration::from_secs(61)).is_ok());
        // After the lock ran out, one more mistake is only the first again.
        assert!(guard.failure("alice", t0 + Duration::from_secs(61), 5, lock).is_ok());
    }

    #[test]
    fn the_window_forgets_old_events() {
        let limiter = WindowLimiter::default();
        let t0 = Instant::now();
        let window = Duration::from_secs(10);
        for _ in 0..3 {
            assert!(limiter.hit("u", t0, 3, window).is_ok());
        }
        let wait = limiter.hit("u", t0 + Duration::from_secs(1), 3, window).unwrap_err();
        assert!((9..=10).contains(&wait), "got {wait}");
        assert!(limiter.hit("other", t0, 3, window).is_ok());
        assert!(limiter.hit("u", t0 + Duration::from_secs(11), 3, window).is_ok());
    }

    #[test]
    fn refused_events_do_not_extend_the_wait() {
        let limiter = WindowLimiter::default();
        let t0 = Instant::now();
        let window = Duration::from_secs(10);
        assert!(limiter.hit("u", t0, 1, window).is_ok());
        for second in 1..9 {
            assert!(limiter.hit("u", t0 + Duration::from_secs(second), 1, window).is_err());
        }
        assert!(limiter.hit("u", t0 + Duration::from_secs(10), 1, window).is_ok());
    }
}
