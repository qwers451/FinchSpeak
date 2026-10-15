//! Adaptive Jitter Buffer with 16-bit sequence number extension and Opus PLC.
//!
//! Converts wrapping 16-bit sequence numbers into monotonic 64-bit indexes,
//! dynamically adapts buffer delay (40 ms – 120 ms) based on RFC 3550 inter-arrival jitter,
//! and invokes Opus Packet Loss Concealment (PLC) when packets are delayed or lost.

use std::collections::BTreeMap;
use std::time::Instant;

use bytes::Bytes;
use finch_protocol::media::MediaHeader;

use crate::opus::OpusVoiceDecoder;
use crate::resampler::FRAME_SIZE_48K;

/// Minimum jitter buffer delay (2 packets = 40 ms).
pub const MIN_BUFFER_PACKETS: usize = 2;

/// Maximum jitter buffer delay (6 packets = 120 ms).
pub const MAX_BUFFER_PACKETS: usize = 6;

/// Result of popping an audio frame from the jitter buffer.
#[derive(Debug, PartialEq, Eq)]
pub enum PlayoutResult {
    /// Normal packet successfully decoded.
    Normal,
    /// Packet was missing; synthesized smoothly via Opus PLC.
    Concealed,
    /// Buffer is still gathering initial packets before starting playout.
    Buffering,
    /// Stream is currently silent or empty.
    Empty,
}

/// A stored packet in the jitter buffer queue.
struct QueuedPacket {
    _timestamp: u32,
    payload: Bytes,
    flags: u8,
}

/// Adaptive Jitter Buffer for one speaker.
pub struct JitterBuffer {
    last_seq: Option<u16>,
    last_extended_seq: u64,
    next_playout_seq: Option<u64>,
    queue: BTreeMap<u64, QueuedPacket>,
    decoder: OpusVoiceDecoder,

    // RFC 3550 Jitter estimation
    last_transit: Option<(Instant, u32)>,
    jitter_ms: f64,
    target_packets: usize,

    // Packet loss tracking
    total_expected: u64,
    total_lost: u64,

    /// True until `target_packets` are queued (start of a talk spurt or after an underrun).
    priming: bool,
    /// While re-buffering after an underrun, fill with PLC instead of silence.
    conceal_while_priming: bool,
}

impl JitterBuffer {
    /// Create a new jitter buffer for a remote participant.
    pub fn new() -> Result<Self, crate::opus::AudioOpusError> {
        let decoder = OpusVoiceDecoder::new()?;
        Ok(Self {
            last_seq: None,
            last_extended_seq: 0,
            next_playout_seq: None,
            queue: BTreeMap::new(),
            decoder,
            last_transit: None,
            jitter_ms: 0.0,
            target_packets: MIN_BUFFER_PACKETS,
            total_expected: 0,
            total_lost: 0,
            priming: true,
            conceal_while_priming: false,
        })
    }

    /// Push an arriving media packet into the jitter buffer.
    pub fn push(&mut self, header: &MediaHeader, payload: Bytes, now: Instant) {
        // Expand 16-bit sequence to 64-bit monotonic sequence
        let extended_seq = match self.last_seq {
            None => {
                self.last_seq = Some(header.sequence);
                self.last_extended_seq = header.sequence as u64;
                header.sequence as u64
            }
            Some(last_seq) => {
                // Signed 16-bit difference handles sequence rollover seamlessly
                let delta = header.sequence.wrapping_sub(last_seq) as i16;
                let ext = (self.last_extended_seq as i64 + delta as i64) as u64;

                // Update last seen if this is the newest packet
                if delta > 0 {
                    self.last_seq = Some(header.sequence);
                    self.last_extended_seq = ext;
                }
                ext
            }
        };

        // RFC 3550 inter-arrival jitter calculation
        if let Some((last_arrival, last_timestamp)) = self.last_transit {
            let arrival_diff_ms = now.duration_since(last_arrival).as_secs_f64() * 1000.0;
            // MediaHeader timestamps are in milliseconds.
            let timestamp_diff_ms = (header.timestamp.wrapping_sub(last_timestamp)) as f64;

            let d = (arrival_diff_ms - timestamp_diff_ms).abs();
            self.jitter_ms += (d - self.jitter_ms) / 16.0;

            // Adapt target buffer depth based on jitter
            let jitter_ms = self.jitter_ms;
            let needed_packets = (jitter_ms / 20.0).ceil() as usize + 1;
            self.target_packets = needed_packets.clamp(MIN_BUFFER_PACKETS, MAX_BUFFER_PACKETS);
        }
        self.last_transit = Some((now, header.timestamp));

        // A packet whose slot was already played or concealed can no longer be used.
        if let Some(next) = self.next_playout_seq
            && extended_seq < next
        {
            return;
        }

        // Insert into queue
        self.queue.insert(
            extended_seq,
            QueuedPacket {
                _timestamp: header.timestamp,
                payload,
                flags: header.flags,
            },
        );

        // Bound latency: if the sender runs ahead of playout, drop the oldest packets.
        while self.queue.len() > MAX_BUFFER_PACKETS {
            self.queue.pop_first();
        }
        if let (Some(next), Some(&oldest)) = (self.next_playout_seq, self.queue.keys().next())
            && next < oldest
        {
            self.next_playout_seq = Some(oldest);
        }

        // Initialize playout pointer if starting fresh
        if self.next_playout_seq.is_none()
            && let Some(&first_seq) = self.queue.keys().next()
        {
            self.next_playout_seq = Some(first_seq);
        }
    }

    /// Read and decode the next 20 ms frame (960 samples @ 48 kHz).
    /// If the required packet has not arrived, synthesizes speech via Opus PLC.
    pub fn pop_frame(&mut self, output: &mut [f32; FRAME_SIZE_48K]) -> PlayoutResult {
        let playout_seq = match self.next_playout_seq {
            Some(seq) => seq,
            None => {
                output.fill(0.0);
                return PlayoutResult::Empty;
            }
        };

        // Buffer `target_packets` at the start of every talk spurt and after an underrun
        if self.priming {
            if self.queue.len() < self.target_packets {
                if self.conceal_while_priming {
                    let _ = self.decoder.decode_plc(output);
                    return PlayoutResult::Concealed;
                }
                output.fill(0.0);
                return PlayoutResult::Buffering;
            }
            self.priming = false;
            self.conceal_while_priming = false;
        }

        // Underrun: nothing queued at all means playout is ahead of the network, not that the
        // packet was lost. Hold position and re-buffer to the current target instead.
        if self.queue.is_empty() {
            let _ = self.decoder.decode_plc(output);
            self.priming = true;
            self.conceal_while_priming = true;
            return PlayoutResult::Concealed;
        }

        self.total_expected += 1;

        if let Some(packet) = self.queue.remove(&playout_seq) {
            // Check if this was silence end packet
            let is_silence = (packet.flags & finch_protocol::media::FLAG_SILENCE) != 0;

            if self.decoder.decode(&packet.payload, output).is_ok() {
                self.next_playout_seq = Some(playout_seq + 1);

                if is_silence && self.queue.is_empty() {
                    // Reset playout on silence end; the next spurt is buffered again
                    self.next_playout_seq = None;
                    self.last_transit = None;
                    self.priming = true;
                }
                PlayoutResult::Normal
            } else {
                let _ = self.decoder.decode_plc(output);
                self.next_playout_seq = Some(playout_seq + 1);
                PlayoutResult::Concealed
            }
        } else {
            // Packet loss: call Opus PLC to smooth over the gap
            self.total_lost += 1;
            let _ = self.decoder.decode_plc(output);
            self.next_playout_seq = Some(playout_seq + 1);

            // If we have fallen too far behind incoming packets, resynchronize
            if let Some(&newest_seq) = self.queue.keys().next_back()
                && newest_seq.saturating_sub(playout_seq) > (MAX_BUFFER_PACKETS as u64 * 3)
                && let Some(&oldest_queued) = self.queue.keys().next()
            {
                self.next_playout_seq = Some(oldest_queued);
            }

            PlayoutResult::Concealed
        }
    }

    /// Number of packets currently waiting in the playout queue.
    pub fn buffered_len(&self) -> usize {
        self.queue.len()
    }

    /// Current loss percentage (0.0 to 1.0).
    pub fn loss_rate(&self) -> f64 {
        if self.total_expected == 0 {
            0.0
        } else {
            self.total_lost as f64 / self.total_expected as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opus::OpusVoiceEncoder;

    #[test]
    fn test_jitter_buffer_sequence_rollover() {
        let mut buffer = JitterBuffer::new().unwrap();
        let now = Instant::now();

        let mut encoder = OpusVoiceEncoder::new(64).unwrap();
        let dummy = [0.1f32; FRAME_SIZE_48K];
        let opus_data = encoder.encode(&dummy).unwrap();

        // Sequence just before u16 overflow (65534, 65535, 0, 1)
        let seqs = [65534u16, 65535u16, 0u16, 1u16];

        for &s in &seqs {
            let header = MediaHeader::new_audio(1, 1, s, (s as u32) * 20, false, false);
            buffer.push(&header, opus_data.clone(), now);
        }

        assert_eq!(buffer.queue.len(), 4);
        let keys: Vec<u64> = buffer.queue.keys().copied().collect();
        assert_eq!(keys, vec![65534, 65535, 65536, 65537]);
    }

    #[test]
    fn test_jitter_buffer_playout_and_plc() {
        let mut buffer = JitterBuffer::new().unwrap();
        let now = Instant::now();

        let mut encoder = OpusVoiceEncoder::new(64).unwrap();
        let dummy = [0.2f32; FRAME_SIZE_48K];
        let opus_data = encoder.encode(&dummy).unwrap();

        // Push packet 0 and packet 2 (packet 1 dropped in network)
        let h0 = MediaHeader::new_audio(1, 1, 0, 0, false, false);
        buffer.push(&h0, opus_data.clone(), now);

        let h2 = MediaHeader::new_audio(1, 1, 2, 40, false, false);
        buffer.push(&h2, opus_data.clone(), now);

        let mut out = [0.0f32; FRAME_SIZE_48K];

        // Playout packet 0
        let r0 = buffer.pop_frame(&mut out);
        assert_eq!(r0, PlayoutResult::Normal);

        // Playout packet 1 (missing => PLC concealment)
        let r1 = buffer.pop_frame(&mut out);
        assert_eq!(r1, PlayoutResult::Concealed);

        // Playout packet 2 (arrived)
        let r2 = buffer.pop_frame(&mut out);
        assert_eq!(r2, PlayoutResult::Normal);
    }

    fn opus_frame() -> Bytes {
        let mut encoder = OpusVoiceEncoder::new(64).unwrap();
        encoder.encode(&[0.1f32; FRAME_SIZE_48K]).unwrap()
    }

    fn push_seq(buffer: &mut JitterBuffer, seq: u16, silence: bool, payload: &Bytes) {
        let header = MediaHeader::new_audio(1, 1, seq, seq as u32 * 20, silence, false);
        buffer.push(&header, payload.clone(), Instant::now());
    }

    /// A packet arriving after its slot was played (or concealed) is useless and must not
    /// linger in the queue (AUD-022).
    #[test]
    fn test_late_packet_is_dropped() {
        let mut buffer = JitterBuffer::new().unwrap();
        let payload = opus_frame();
        let mut out = [0.0f32; FRAME_SIZE_48K];

        push_seq(&mut buffer, 10, false, &payload);
        push_seq(&mut buffer, 12, false, &payload);
        assert_eq!(buffer.pop_frame(&mut out), PlayoutResult::Normal); // 10
        assert_eq!(buffer.pop_frame(&mut out), PlayoutResult::Concealed); // 11 lost
        push_seq(&mut buffer, 11, false, &payload); // 11 arrives too late
        assert_eq!(buffer.buffered_len(), 1, "only 12 should remain queued");
    }

    /// The queue never grows beyond MAX_BUFFER_PACKETS, even if the sender runs ahead
    /// of playout (AUD-022).
    #[test]
    fn test_queue_is_bounded() {
        let mut buffer = JitterBuffer::new().unwrap();
        let payload = opus_frame();
        for seq in 0..50u16 {
            push_seq(&mut buffer, seq, false, &payload);
        }
        assert!(
            buffer.buffered_len() <= MAX_BUFFER_PACKETS,
            "queue holds {} packets",
            buffer.buffered_len()
        );
    }

    /// After a silence reset the next talk spurt is buffered again instead of playing
    /// the first packet immediately (AUD-022).
    #[test]
    fn test_reprimes_after_silence() {
        let mut buffer = JitterBuffer::new().unwrap();
        let payload = opus_frame();
        let mut out = [0.0f32; FRAME_SIZE_48K];

        push_seq(&mut buffer, 0, false, &payload);
        push_seq(&mut buffer, 1, true, &payload); // last packet of the spurt
        buffer.pop_frame(&mut out);
        buffer.pop_frame(&mut out);

        push_seq(&mut buffer, 30, false, &payload); // next spurt starts
        assert_eq!(buffer.pop_frame(&mut out), PlayoutResult::Buffering);
    }

    /// Packets arrive with 0..60 ms random delay. The buffer must grow its delay to absorb the
    /// jitter instead of declaring late packets lost (AUD-022).
    #[test]
    fn test_absorbs_random_network_delay() {
        let mut buffer = JitterBuffer::new().unwrap();
        let payload = opus_frame();
        let mut out = [0.0f32; FRAME_SIZE_48K];
        let start = Instant::now();

        // Deterministic pseudo-random delays in 0..=60 ms (LCG).
        let mut x: u64 = 42;
        let mut arrivals: Vec<(u64, u16)> = (0..250u16)
            .map(|seq| {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (seq as u64 * 20 + (x >> 33) % 61, seq)
            })
            .collect();
        arrivals.sort();

        let mut next = 0;
        for tick in 0..280u64 {
            let now_ms = tick * 20;
            while next < arrivals.len() && arrivals[next].0 <= now_ms {
                let (at, seq) = arrivals[next];
                let header = MediaHeader::new_audio(1, 1, seq, seq as u32 * 20, false, false);
                buffer.push(&header, payload.clone(), start + std::time::Duration::from_millis(at));
                next += 1;
            }
            buffer.pop_frame(&mut out);
        }
        assert!(buffer.loss_rate() < 0.05, "loss rate {:.2} with no real losses", buffer.loss_rate());
    }
}
