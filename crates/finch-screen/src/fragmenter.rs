//! H.264 video frame fragmentation and receiver reassembly.
//!
//! - IDR keyframes are sent over QUIC Uni-Streams with a 20-byte `MediaHeader`.
//! - Delta (P) frames are sliced into fragments up to 1160 bytes and sent over QUIC Datagrams.
//! - The receiver assembles fragments within a 40ms timeout and drops intermediate
//!   delta frames while awaiting an incoming keyframe stream.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use finch_protocol::media::{MediaHeader, MediaPacket, MAX_DATAGRAM_PAYLOAD};

/// Default timeout for waiting for all fragments of a delta frame (40 ms).
pub const DELTA_FRAME_TIMEOUT: Duration = Duration::from_millis(40);

/// An assembled video frame ready to be passed to WebCodecs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledFrame {
    pub frame_id: u16,
    pub is_keyframe: bool,
    pub timestamp: u32,
    pub data: Bytes,
}

/// Slices an encoded H.264 P-frame into datagram-sized media packets.
///
/// Returns no packets for an empty frame or one that needs more than 255 fragments (the
/// header's fragment count is a `u8`); use [`slice_p_frame_with_chunk`] to tell them apart.
pub fn slice_p_frame(
    channel_id: u32,
    sender_peer_id: u32,
    sequence_start: u16,
    frame_id: u16,
    timestamp: u32,
    frame_data: &[u8],
    is_e2ee: bool,
) -> Vec<MediaPacket> {
    slice_p_frame_with_chunk(
        channel_id,
        sender_peer_id,
        sequence_start,
        frame_id,
        timestamp,
        frame_data,
        is_e2ee,
        MAX_DATAGRAM_PAYLOAD,
    )
    .unwrap_or_default()
}

/// Like [`slice_p_frame`] with a caller-chosen payload size per datagram.
///
/// `None` when the frame would need more than 255 fragments (or `chunk_size` is zero), so the
/// fragment count in the header can never silently wrap.
#[allow(clippy::too_many_arguments)]
pub fn slice_p_frame_with_chunk(
    channel_id: u32,
    sender_peer_id: u32,
    sequence_start: u16,
    frame_id: u16,
    timestamp: u32,
    frame_data: &[u8],
    is_e2ee: bool,
    chunk_size: usize,
) -> Option<Vec<MediaPacket>> {
    if chunk_size == 0 {
        return None;
    }
    if frame_data.is_empty() {
        return Some(Vec::new());
    }

    let total_fragments = u8::try_from(frame_data.len().div_ceil(chunk_size)).ok()?;
    let mut packets = Vec::with_capacity(total_fragments as usize);

    for (i, chunk) in frame_data.chunks(chunk_size).enumerate() {
        let seq = sequence_start.wrapping_add(i as u16);
        let header = MediaHeader::new_video_delta(
            channel_id,
            sender_peer_id,
            seq,
            timestamp,
            frame_id,
            i as u8,
            total_fragments,
            is_e2ee,
        );

        packets.push(MediaPacket::new(header, Bytes::copy_from_slice(chunk)));
    }

    Some(packets)
}

/// State for a frame currently being assembled on the receiver.
struct PendingFrame {
    total_fragments: u8,
    received_count: u8,
    fragments: Vec<Option<Bytes>>,
    first_received_at: Instant,
    timestamp: u32,
}

/// Receiver-side frame assembler with keyframe-blocking synchronization.
pub struct FrameAssembler {
    /// Pending delta frames indexed by frame_id.
    pending_frames: HashMap<u16, PendingFrame>,
    /// Set to true while an incoming Uni-Stream keyframe is being read.
    waiting_for_keyframe: bool,
    /// Last received keyframe frame_id.
    last_keyframe_id: Option<u16>,
}

impl Default for FrameAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameAssembler {
    pub fn new() -> Self {
        Self {
            pending_frames: HashMap::new(),
            waiting_for_keyframe: false,
            last_keyframe_id: None,
        }
    }

    /// Called when the receiver detects an incoming QUIC Uni-Stream carrying a keyframe.
    /// Sets the blocking flag so arriving delta frames are discarded until the IDR is ready.
    pub fn on_keyframe_stream_started(&mut self) {
        self.waiting_for_keyframe = true;
        // Clear any old pending delta frames since they cannot be decoded without the new IDR
        self.pending_frames.clear();
    }

    /// Called when a complete keyframe has been received from the Uni-Stream.
    pub fn on_keyframe_received(
        &mut self,
        frame_id: u16,
        timestamp: u32,
        keyframe_data: Bytes,
    ) -> AssembledFrame {
        self.waiting_for_keyframe = false;
        self.last_keyframe_id = Some(frame_id);
        self.pending_frames.clear();

        AssembledFrame {
            frame_id,
            is_keyframe: true,
            timestamp,
            data: keyframe_data,
        }
    }

    /// Process an incoming delta frame fragment from a datagram.
    /// Returns `Some(AssembledFrame)` when all fragments for the frame have arrived.
    pub fn push_fragment(
        &mut self,
        header: &MediaHeader,
        payload: Bytes,
        now: Instant,
    ) -> Option<AssembledFrame> {
        // Drop deltas if we are currently awaiting a keyframe
        if self.waiting_for_keyframe {
            return None;
        }

        // If this delta frame has an older frame_id than our current keyframe, drop it
        if let Some(key_id) = self.last_keyframe_id {
            let delta = header.frame_id.wrapping_sub(key_id) as i16;
            if delta < 0 {
                return None;
            }
        }

        let total = header.total_fragments;
        let index = header.fragment_index as usize;

        if total == 0 || index >= total as usize {
            return None;
        }

        // Single-fragment frame optimization
        if total == 1 {
            return Some(AssembledFrame {
                frame_id: header.frame_id,
                is_keyframe: false,
                timestamp: header.timestamp,
                data: payload,
            });
        }

        let pending = self
            .pending_frames
            .entry(header.frame_id)
            .or_insert_with(|| PendingFrame {
                total_fragments: total,
                received_count: 0,
                fragments: vec![None; total as usize],
                first_received_at: now,
                timestamp: header.timestamp,
            });

        // Store fragment if not already received
        if index < pending.fragments.len() && pending.fragments[index].is_none() {
            pending.fragments[index] = Some(payload);
            pending.received_count += 1;
        }

        // Check if all fragments arrived
        if pending.received_count == pending.total_fragments
            && let Some(mut complete) = self.pending_frames.remove(&header.frame_id)
        {
            let total_size: usize = complete
                .fragments
                .iter()
                .map(|f| f.as_ref().map(|b| b.len()).unwrap_or(0))
                .sum();

            let mut assembled = BytesMut::with_capacity(total_size);
            for b in complete.fragments.drain(..).flatten() {
                assembled.extend_from_slice(&b);
            }

            return Some(AssembledFrame {
                frame_id: header.frame_id,
                is_keyframe: false,
                timestamp: complete.timestamp,
                data: assembled.freeze(),
            });
        }

        None
    }

    /// Prune incomplete frames older than `timeout`.
    /// Returns the frame_ids of dropped frames so a RequestKeyframe can be sent.
    pub fn prune_stale(&mut self, now: Instant, timeout: Duration) -> Vec<u16> {
        let mut dropped = Vec::new();

        self.pending_frames.retain(|&frame_id, frame| {
            if now.duration_since(frame.first_received_at) > timeout {
                dropped.push(frame_id);
                false
            } else {
                true
            }
        });

        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slice_and_reassemble() {
        let mut assembler = FrameAssembler::new();
        let payload = vec![0x42u8; 3000]; // 3000 bytes => 3 fragments of max 1160 (1160 + 1160 + 680)

        let packets = slice_p_frame(1, 10, 100, 5, 90_000, &payload, false);
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[0].header.total_fragments, 3);
        assert_eq!(packets[0].header.fragment_index, 0);
        assert_eq!(packets[1].header.fragment_index, 1);
        assert_eq!(packets[2].header.fragment_index, 2);

        let now = Instant::now();
        // Send fragment 1 first (out of order)
        let res1 = assembler.push_fragment(&packets[1].header, packets[1].payload.clone(), now);
        assert!(res1.is_none());

        // Send fragment 0
        let res2 = assembler.push_fragment(&packets[0].header, packets[0].payload.clone(), now);
        assert!(res2.is_none());

        // Send fragment 2 (completes frame)
        let res3 = assembler.push_fragment(&packets[2].header, packets[2].payload.clone(), now);
        assert!(res3.is_some());

        let assembled = res3.unwrap();
        assert_eq!(assembled.frame_id, 5);
        assert!(!assembled.is_keyframe);
        assert_eq!(assembled.data.len(), 3000);
        assert_eq!(&assembled.data[..], &payload[..]);
    }

    #[test]
    fn test_keyframe_blocking() {
        let mut assembler = FrameAssembler::new();
        let packets = slice_p_frame(1, 10, 100, 5, 90_000, &[1, 2, 3], false);

        // A keyframe stream has started
        assembler.on_keyframe_stream_started();

        // Arriving delta frame should be dropped while awaiting keyframe
        let res = assembler.push_fragment(&packets[0].header, packets[0].payload.clone(), Instant::now());
        assert!(res.is_none());

        // Now keyframe arrives
        let kf = assembler.on_keyframe_received(6, 90_033, Bytes::from_static(&[0x00, 0x00, 0x00, 0x01, 0x67]));
        assert!(kf.is_keyframe);
        assert_eq!(kf.frame_id, 6);

        // Subsequent delta with frame_id >= 6 should be accepted
        let packets_next = slice_p_frame(1, 10, 105, 7, 90_066, &[4, 5, 6], false);
        let res2 = assembler.push_fragment(&packets_next[0].header, packets_next[0].payload.clone(), Instant::now());
        assert!(res2.is_some());
        assert_eq!(res2.unwrap().frame_id, 7);
    }
}
