//! Link quality of a remote speaker, derived from recent packet loss.

use std::collections::HashMap;

/// Below this share of lost packets the link is good.
pub const GOOD_BELOW: f64 = 0.03;
/// Below this share it is fair; at or above it, poor.
pub const FAIR_BELOW: f64 = 0.08;
/// A window with fewer packets than this says too little to judge: it keeps the last verdict.
pub const MIN_PACKETS_FOR_VERDICT: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkQuality {
    Good,
    Fair,
    Poor,
}

impl LinkQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkQuality::Good => "good",
            LinkQuality::Fair => "fair",
            LinkQuality::Poor => "poor",
        }
    }
}

/// Classify a loss rate (0.0..=1.0).
pub fn classify(loss: f64) -> LinkQuality {
    if loss < GOOD_BELOW {
        LinkQuality::Good
    } else if loss < FAIR_BELOW {
        LinkQuality::Fair
    } else {
        LinkQuality::Poor
    }
}

/// Turns cumulative `(expected, lost)` counters into a verdict per window (the loss of the last
/// interval, not of the whole call, so a link that recovers is shown as recovered).
#[derive(Default)]
pub struct QualityTracker {
    last: HashMap<u32, (u64, u64)>,
    verdict: HashMap<u32, LinkQuality>,
}

impl QualityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the current counters of every speaker; returns the verdict of each one.
    /// Speakers that are no longer listed are forgotten.
    pub fn update(&mut self, counters: &[(u32, u64, u64)]) -> Vec<(u32, LinkQuality, f64)> {
        self.last.retain(|peer, _| counters.iter().any(|(p, _, _)| p == peer));
        self.verdict.retain(|peer, _| counters.iter().any(|(p, _, _)| p == peer));
        let mut result = Vec::with_capacity(counters.len());
        for &(peer, expected, lost) in counters {
            let (previous_expected, previous_lost) = self.last.insert(peer, (expected, lost)).unwrap_or((0, 0));
            // Counters only grow; a smaller value means the speaker was replaced.
            let (window_expected, window_lost) = if expected >= previous_expected && lost >= previous_lost {
                (expected - previous_expected, lost - previous_lost)
            } else {
                (expected, lost)
            };
            let loss = if window_expected == 0 { 0.0 } else { window_lost as f64 / window_expected as f64 };
            let quality = if window_expected >= MIN_PACKETS_FOR_VERDICT {
                let quality = classify(loss);
                self.verdict.insert(peer, quality);
                quality
            } else {
                *self.verdict.get(&peer).unwrap_or(&LinkQuality::Good)
            };
            result.push((peer, quality, loss));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_split_at_three_and_eight_percent() {
        assert_eq!(classify(0.0), LinkQuality::Good);
        assert_eq!(classify(0.029), LinkQuality::Good);
        assert_eq!(classify(0.03), LinkQuality::Fair);
        assert_eq!(classify(0.079), LinkQuality::Fair);
        assert_eq!(classify(0.08), LinkQuality::Poor);
        assert_eq!(classify(1.0), LinkQuality::Poor);
    }

    #[test]
    fn a_window_judges_recent_loss_only() {
        let mut tracker = QualityTracker::new();
        // A bad start: 20 % lost of 100.
        assert_eq!(tracker.update(&[(1, 100, 20)])[0].1, LinkQuality::Poor);
        // The next second is clean: 50 more packets, none lost -> good again although the total is still high.
        let verdict = tracker.update(&[(1, 150, 20)]);
        assert_eq!(verdict[0].1, LinkQuality::Good);
        assert_eq!(verdict[0].2, 0.0);
    }

    #[test]
    fn a_quiet_speaker_keeps_the_last_verdict() {
        let mut tracker = QualityTracker::new();
        tracker.update(&[(1, 100, 30)]);
        // Three packets in the next second: too few to say anything new.
        assert_eq!(tracker.update(&[(1, 103, 30)])[0].1, LinkQuality::Poor);
        // A speaker never judged starts as good.
        assert_eq!(tracker.update(&[(1, 103, 30), (2, 2, 2)])[1].1, LinkQuality::Good);
    }

    #[test]
    fn a_replaced_speaker_does_not_underflow() {
        let mut tracker = QualityTracker::new();
        tracker.update(&[(1, 500, 5)]);
        // The counters restarted (same peer id, new stream).
        let verdict = tracker.update(&[(1, 40, 0)]);
        assert_eq!(verdict[0].1, LinkQuality::Good);
    }

    #[test]
    fn speakers_that_left_are_forgotten() {
        let mut tracker = QualityTracker::new();
        tracker.update(&[(1, 100, 50), (2, 100, 0)]);
        tracker.update(&[(2, 200, 0)]);
        // Peer 1 comes back as a new speaker: judged from scratch.
        assert_eq!(tracker.update(&[(1, 5, 0), (2, 300, 0)])[0].1, LinkQuality::Good);
    }
}
