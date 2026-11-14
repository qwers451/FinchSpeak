//! Rate limits: wrong passwords, registrations, and message floods.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;

use finch_protocol::media::MAX_KEYFRAME_BYTES;

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

/// Media one participant may send, in independent token buckets.
///
/// Datagrams (voice, screen deltas) are charged to the bytes and datagrams buckets. Keyframes,
/// which arrive on uni-streams, are charged to their own bytes bucket, whose burst is one maximum
/// keyframe: a keyframe never delays datagrams, and datagrams never delay a keyframe.
///
/// Pure: the caller passes the clock, so tests drive time directly. A refused message takes
/// nothing from any bucket.
pub struct MediaRateLimiter {
    bytes_per_sec: f64,
    burst_bytes: f64,
    bytes: f64,
    packets_per_sec: f64,
    burst_packets: f64,
    packets: f64,
    keyframe_bytes_per_sec: f64,
    keyframe_burst: f64,
    keyframe_bytes: f64,
    last: Instant,
}

impl MediaRateLimiter {
    /// Full buckets: a new participant may send one burst at once.
    pub fn new(limits: &LimitsConfig, now: Instant) -> Self {
        let burst_bytes = limits.media_burst_bytes as f64;
        let burst_packets = limits.media_packets_per_sec as f64;
        let keyframe_burst = MAX_KEYFRAME_BYTES as f64;
        Self {
            bytes_per_sec: limits.media_bytes_per_sec as f64,
            burst_bytes,
            bytes: burst_bytes,
            packets_per_sec: burst_packets,
            burst_packets,
            packets: burst_packets,
            keyframe_bytes_per_sec: limits.media_keyframe_bytes_per_sec as f64,
            keyframe_burst,
            keyframe_bytes: keyframe_burst,
            last: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        // A clock that goes back refills nothing and is not an error.
        if now > self.last {
            let elapsed = now.duration_since(self.last).as_secs_f64();
            self.bytes = (self.bytes + elapsed * self.bytes_per_sec).min(self.burst_bytes);
            self.packets = (self.packets + elapsed * self.packets_per_sec).min(self.burst_packets);
            self.keyframe_bytes =
                (self.keyframe_bytes + elapsed * self.keyframe_bytes_per_sec).min(self.keyframe_burst);
            self.last = now;
        }
    }

    /// Charge one keyframe of `bytes` to the keyframe bucket. `false` when it is over the limit.
    pub fn allow_keyframe(&mut self, now: Instant, bytes: usize) -> bool {
        self.refill(now);
        let bytes = bytes as f64;
        if self.keyframe_bytes < bytes {
            return false;
        }
        self.keyframe_bytes -= bytes;
        true
    }

    /// Charge one datagram of `bytes`. `false` when it is over the limit (nothing is taken then).
    pub fn allow(&mut self, now: Instant, bytes: usize) -> bool {
        self.refill(now);
        let bytes = bytes as f64;
        if self.bytes < bytes || self.packets < 1.0 {
            return false;
        }
        self.bytes -= bytes;
        self.packets -= 1.0;
        true
    }
}

/// Slots for connections that have not signed in yet: at most `per_ip` per client address and
/// `total` overall. A permit is taken right after the handshake and returned when the connection
/// signs in or closes, whichever comes first.
#[derive(Clone)]
pub struct PreAuthSlots {
    inner: Arc<PreAuthInner>,
}

struct PreAuthInner {
    per_ip: usize,
    total: usize,
    open: AtomicUsize,
    per_addr: DashMap<IpAddr, usize>,
}

/// Limits every connection meets before it signs in: how long it may take, and the slot pool.
#[derive(Clone)]
pub struct PreAuth {
    pub timeout: Duration,
    pub slots: PreAuthSlots,
}

impl PreAuth {
    pub fn from_limits(limits: &LimitsConfig) -> Self {
        Self {
            timeout: Duration::from_secs(limits.preauth_timeout_secs),
            slots: PreAuthSlots::new(limits.preauth_per_ip, limits.preauth_total),
        }
    }
}

/// One unauthenticated slot. Dropping it frees the slot.
pub struct PreAuthPermit {
    inner: Arc<PreAuthInner>,
    ip: IpAddr,
}

impl PreAuthSlots {
    pub fn new(per_ip: usize, total: usize) -> Self {
        Self {
            inner: Arc::new(PreAuthInner {
                per_ip,
                total,
                open: AtomicUsize::new(0),
                per_addr: DashMap::new(),
            }),
        }
    }

    /// A slot for a connection from `ip`, or `None` when its address or the server is full.
    /// IPv4-mapped IPv6 addresses count as their IPv4 address.
    pub fn try_acquire(&self, ip: IpAddr) -> Option<PreAuthPermit> {
        let ip = ip.to_canonical();
        let inner = &self.inner;
        // The total is reserved first, so a refusal here never leaves a per-address count behind.
        if inner.open.fetch_add(1, Ordering::SeqCst) >= inner.total {
            inner.open.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        {
            let mut count = inner.per_addr.entry(ip).or_insert(0);
            if *count < inner.per_ip {
                *count += 1;
                return Some(PreAuthPermit { inner: self.inner.clone(), ip });
            }
        }
        inner.open.fetch_sub(1, Ordering::SeqCst);
        None
    }

    /// Slots currently held.
    pub fn open(&self) -> usize {
        self.inner.open.load(Ordering::SeqCst)
    }
}

impl Drop for PreAuthPermit {
    fn drop(&mut self) {
        if let Some(mut count) = self.inner.per_addr.get_mut(&self.ip) {
            *count = count.saturating_sub(1);
        }
        self.inner.per_addr.remove_if(&self.ip, |_, count| *count == 0);
        self.inner.open.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod preauth_tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn v4(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, last))
    }

    #[test]
    fn one_address_gets_per_ip_slots_and_no_more() {
        let slots = PreAuthSlots::new(2, 10);
        let first = slots.try_acquire(v4(1)).expect("first slot");
        let _second = slots.try_acquire(v4(1)).expect("second slot");
        assert!(slots.try_acquire(v4(1)).is_none(), "a third from the same address is refused");
        assert!(slots.try_acquire(v4(2)).is_some(), "another address is unaffected");
        drop(first);
        assert!(slots.try_acquire(v4(1)).is_some(), "dropping a permit frees its slot");
    }

    #[test]
    fn the_total_cap_applies_across_addresses() {
        let slots = PreAuthSlots::new(8, 3);
        let held: Vec<_> = (1..=3).map(|i| slots.try_acquire(v4(i)).expect("within the total")).collect();
        assert!(slots.try_acquire(v4(4)).is_none(), "a fourth address is refused by the total cap");
        assert_eq!(slots.open(), 3);
        drop(held);
        assert_eq!(slots.open(), 0);
        assert!(slots.try_acquire(v4(4)).is_some(), "capacity returns when the permits drop");
    }

    #[test]
    fn a_refusal_by_the_total_cap_takes_no_per_address_slot() {
        let slots = PreAuthSlots::new(1, 1);
        let held = slots.try_acquire(v4(1)).expect("first");
        assert!(slots.try_acquire(v4(2)).is_none(), "the total is full");
        drop(held);
        assert!(slots.try_acquire(v4(2)).is_some(), "the refused attempt left no per-address slot behind");
    }

    #[test]
    fn ipv4_mapped_ipv6_counts_as_the_ipv4_address() {
        let slots = PreAuthSlots::new(1, 10);
        let _plain = slots.try_acquire(v4(7)).expect("plain IPv4");
        let mapped = IpAddr::V6(Ipv4Addr::new(10, 0, 0, 7).to_ipv6_mapped());
        assert!(slots.try_acquire(mapped).is_none(), "the mapped form is the same client");
    }

    #[test]
    fn full_ipv6_addresses_are_counted_separately() {
        let slots = PreAuthSlots::new(1, 10);
        let a = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        let b = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2));
        let _held = slots.try_acquire(a).expect("first v6 address");
        assert!(slots.try_acquire(b).is_some(), "a different IPv6 address is a different client");
    }

    #[test]
    fn an_idle_address_leaves_no_entry_behind() {
        let slots = PreAuthSlots::new(1, 10);
        drop(slots.try_acquire(v4(9)).expect("one slot"));
        assert!(slots.inner.per_addr.is_empty());
        assert_eq!(slots.open(), 0);
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
    fn keyframes_have_their_own_bucket_that_datagrams_do_not_touch() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        assert!(bucket.allow(t0, 262_144), "a full datagram burst");
        assert!(!bucket.allow(t0, 1), "datagrams are out");
        assert!(bucket.allow_keyframe(t0, 3_000_000), "a keyframe still has its own budget");
    }

    #[test]
    fn a_keyframe_is_admitted_up_to_a_full_max_keyframe_then_refills_at_its_rate() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        assert!(bucket.allow_keyframe(t0, MAX_KEYFRAME_BYTES), "one maximum keyframe on a full bucket");
        assert!(!bucket.allow_keyframe(t0, 1), "nothing is left in the same instant");
        // The default keyframe rate is 1 MB/s.
        let later = t0 + Duration::from_secs(1);
        assert!(bucket.allow_keyframe(later, 1_000_000));
        assert!(!bucket.allow_keyframe(later, 1));
    }

    #[test]
    fn a_refused_keyframe_takes_nothing_from_the_keyframe_bucket() {
        let t0 = Instant::now();
        let mut bucket = MediaRateLimiter::new(&LimitsConfig::default(), t0);
        assert!(bucket.allow_keyframe(t0, MAX_KEYFRAME_BYTES - 500_000));
        assert!(!bucket.allow_keyframe(t0, 1_000_000), "only 500 000 bytes are left");
        assert!(bucket.allow_keyframe(t0, 500_000), "and the refusal did not take them");
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
