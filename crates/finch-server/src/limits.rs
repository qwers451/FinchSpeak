//! Rate limits: wrong passwords, registrations, and message floods.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::config::LimitsConfig;

const MAX_LOGIN_KEYS: usize = 10_000;

/// The address a REST request came from (added by the HTTPS listener).
#[derive(Debug, Clone, Copy)]
pub struct PeerAddr(pub IpAddr);

/// Counts wrong passwords per key and locks the key for a while after too many.
#[derive(Default)]
pub struct LoginGuard {
    entries: std::sync::Mutex<HashMap<String, Entry>>,
}

#[derive(Default, Clone, Copy)]
struct Entry {
    failures: u32,
    locked_until: Option<Instant>,
    expires_at: Option<Instant>,
}

impl LoginGuard {
    /// `Err(seconds to wait)` while `key` is locked.
    pub fn check(&self, key: &str, now: Instant) -> Result<(), u64> {
        let mut entries = self.entries.lock().map_err(|_| 1u64)?;
        if entries.len() >= MAX_LOGIN_KEYS && !entries.contains_key(key) {
            entries.retain(|_, entry| entry.expires_at.is_some_and(|until| until > now));
            if entries.len() >= MAX_LOGIN_KEYS {
                return Err(1);
            }
        }
        match entries.get(key).and_then(|entry| entry.locked_until) {
            Some(until) if until > now => Err((until - now).as_secs() + 1),
            _ => Ok(()),
        }
    }

    /// A wrong password. Returns `Err(seconds)` if this one locked the key.
    pub fn failure(&self, key: &str, now: Instant, max_failures: u32, lockout: Duration) -> Result<(), u64> {
        let mut entries = self.entries.lock().map_err(|_| 1u64)?;
        if entries.len() >= MAX_LOGIN_KEYS && !entries.contains_key(key) {
            entries.retain(|_, entry| entry.expires_at.is_some_and(|until| until > now));
            if entries.len() >= MAX_LOGIN_KEYS {
                return Err(lockout.as_secs().max(1));
            }
        }
        let entry = entries.entry(key.to_string()).or_default();
        // A lock that has run out starts counting again from zero.
        if entry.locked_until.is_some_and(|until| until <= now) {
            *entry = Entry::default();
        }
        entry.expires_at = Some(now + lockout);
        entry.failures = entry.failures.saturating_add(1);
        if entry.failures >= max_failures {
            entry.locked_until = Some(now + lockout);
            entry.failures = 0;
            return Err(lockout.as_secs().max(1));
        }
        Ok(())
    }

    pub fn success(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key);
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

/// Media one participant may send: a token bucket over bytes and one over datagrams.
///
/// Pure: the caller passes the clock, so tests drive time directly. A datagram passes only when
/// both buckets have room, and a refused datagram takes nothing from either bucket.
pub struct MediaRateLimiter {
    bytes_per_sec: f64,
    burst_bytes: f64,
    bytes: f64,
    packets_per_sec: f64,
    burst_packets: f64,
    packets: f64,
    last: Instant,
}

impl MediaRateLimiter {
    /// A full bucket: a new participant may send one burst at once.
    pub fn new(limits: &LimitsConfig, now: Instant) -> Self {
        let burst_bytes = limits.media_burst_bytes as f64;
        let burst_packets = limits.media_packets_per_sec as f64;
        Self {
            bytes_per_sec: limits.media_bytes_per_sec as f64,
            burst_bytes,
            bytes: burst_bytes,
            packets_per_sec: burst_packets,
            burst_packets,
            packets: burst_packets,
            last: now,
        }
    }

    /// Charge one datagram of `bytes`. `false` when it is over the limit (nothing is taken then).
    pub fn allow(&mut self, now: Instant, bytes: usize) -> bool {
        // A clock that goes back refills nothing and is not an error.
        if now > self.last {
            let elapsed = now.duration_since(self.last).as_secs_f64();
            self.bytes = (self.bytes + elapsed * self.bytes_per_sec).min(self.burst_bytes);
            self.packets = (self.packets + elapsed * self.packets_per_sec).min(self.burst_packets);
            self.last = now;
        }
        let bytes = bytes as f64;
        // A message larger than the burst could never fit whole. It needs a full bucket and then runs
        // the bucket into debt, which later traffic waits out: the long-run rate still holds, and the
        // debt is never more than one message (a keyframe is bounded by the reader's size cap).
        let needed = bytes.min(self.burst_bytes);
        if self.bytes < needed || self.packets < 1.0 {
            return false;
        }
        self.bytes -= bytes;
        self.packets -= 1.0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsuccessful_logins_alone_cannot_grow_the_guard_without_bound() {
        let guard = LoginGuard::default();
        let now = Instant::now();
        for index in 0..10_100 {
            let _ = guard.failure(&format!("unknown-{index}"), now, 5, Duration::from_secs(60));
        }
        assert!(guard.entries.lock().unwrap().len() <= 10_000);
        assert!(guard.check("uncached-victim", now).is_err(), "saturation must reject new keys before password verification");
        assert!(guard.check("later", now + Duration::from_secs(61)).is_ok());
        assert!(guard.failure("later", now + Duration::from_secs(61), 5, Duration::from_secs(60)).is_ok());
        assert!(guard.entries.lock().unwrap().len() < 10_000, "expired attempts must be reclaimed without a successful login");
    }

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

    #[test]
    fn steady_media_under_the_rate_is_never_refused() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        // 1000 bytes every 2 ms is 500 KB/s, well under 1.5 MB/s.
        for step in 0..5_000u32 {
            let now = t0 + Duration::from_millis(2 * u64::from(step));
            assert!(bucket.allow(now, 1000), "packet {step} refused");
        }
    }

    #[test]
    fn the_byte_burst_passes_once_then_refuses_without_charging_the_refusal() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        assert!(bucket.allow(t0, 262_144), "a full burst is allowed");
        assert!(!bucket.allow(t0, 1), "nothing is left in the same instant");
        // 100 ms refills 150 000 bytes: one 100 000 byte packet fits, a second does not.
        let later = t0 + Duration::from_millis(100);
        assert!(bucket.allow(later, 100_000));
        assert!(!bucket.allow(later, 100_000));
    }

    #[test]
    fn refill_is_capped_at_the_burst() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        assert!(bucket.allow(t0, 262_144));
        let idle = t0 + Duration::from_secs(60);
        assert!(bucket.allow(idle, 262_144), "a minute of silence refills exactly one burst");
        assert!(!bucket.allow(idle, 1), "and not more than that");
    }

    #[test]
    fn an_oversized_message_passes_on_a_full_bucket_then_waits_out_its_debt() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        let idle = t0 + Duration::from_secs(60);
        // A 3 MB keyframe is larger than the 256 KB burst: it passes on a full bucket.
        assert!(bucket.allow(idle, 3_000_000), "an oversized message is not refused forever");
        assert!(!bucket.allow(idle, 1), "the bucket is now in debt for that message");
        // The debt (about 2.7 MB) takes about 1.8 s to refill at 1.5 MB/s.
        assert!(!bucket.allow(idle + Duration::from_secs(1), 1));
        assert!(bucket.allow(idle + Duration::from_millis(2500), 1));
    }

    #[test]
    fn an_oversized_message_on_a_part_full_bucket_is_refused() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        assert!(bucket.allow(t0, 200_000));
        // 100 ms refills 150 000 bytes: 250 000 is short of a full burst, so the keyframe waits.
        assert!(!bucket.allow(t0 + Duration::from_millis(100), 3_000_000));
    }

    #[test]
    fn packets_are_limited_separately_from_bytes() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        for _ in 0..2_000 {
            assert!(bucket.allow(t0, 10), "2000 packets fit the first second");
        }
        assert!(!bucket.allow(t0, 10), "the 2001st packet in that second is refused");
        let next = t0 + Duration::from_secs(1);
        for _ in 0..2_000 {
            assert!(bucket.allow(next, 10), "the packet rate refills per second");
        }
    }

    #[test]
    fn a_clock_that_goes_back_neither_refills_nor_panics() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        let t1 = t0 + Duration::from_secs(1);
        assert!(bucket.allow(t1, 262_144));
        assert!(!bucket.allow(t0, 1));
    }
}
