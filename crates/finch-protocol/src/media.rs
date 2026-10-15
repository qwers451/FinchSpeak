//! Media datagram header format for audio and video over QUIC Datagrams and Uni-Streams.
//!
//! ```text
//! ┌──────────┬──────────┬──────────┬──────────┬──────────┬──────────┬──────────┐
//! │ Тип (1Б) │ Канал(4Б)│ Отпр.(4Б)│ Номер(2Б)│ Метка(4Б)│ Кадр(2Б) │Фрагм.(3Б)│
//! └──────────┴──────────┴──────────┴──────────┴──────────┴──────────┴──────────┘
//!            │← ──────────────── Header: 20 bytes ──────────────────── →│
//! ```

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// Size of the media datagram header in bytes.
pub const MEDIA_HEADER_SIZE: usize = 20;

/// Maximum payload size for a single QUIC datagram (excluding 20-byte header).
/// 1180 - 20 = 1160 bytes.
pub const MAX_DATAGRAM_PAYLOAD: usize = 1160;

/// Maximum total datagram size. Guaranteed not to exceed IPv6 minimum MTU.
pub const MAX_DATAGRAM_SIZE: usize = MEDIA_HEADER_SIZE + MAX_DATAGRAM_PAYLOAD; // 1180

/// Bit 0 of `flags`: indicates the last voice packet before silence (hangover end).
pub const FLAG_SILENCE: u8 = 0x01;

/// Bit 1 of `flags`: indicates end-to-end encryption (E2EE) on the payload.
pub const FLAG_E2EE: u8 = 0x02;

/// Type of media carried in a datagram or stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MediaType {
    /// Opus audio frame (typically 20 ms at 48 kHz mono).
    Audio = 0x01,
    /// H.264 IDR keyframe (transmitted in QUIC Uni-Stream header).
    VideoKey = 0x02,
    /// H.264 delta (P) frame fragment (transmitted in datagrams).
    VideoDelta = 0x03,
    /// System audio captured during screen share (Opus encoded stereo).
    ScreenAudio = 0x04,
}

impl MediaType {
    /// Parse a byte into a `MediaType`.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Audio),
            0x02 => Some(Self::VideoKey),
            0x03 => Some(Self::VideoDelta),
            0x04 => Some(Self::ScreenAudio),
            _ => None,
        }
    }
}

/// 20-byte header prepended to every media packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaHeader {
    /// Type of media in this packet (offset 0).
    pub media_type: MediaType,
    /// Voice channel ID mapped to short u32 (offset 1..5).
    pub channel_id: u32,
    /// Session ID of the sender (offset 5..9).
    pub sender_peer_id: u32,
    /// Sequence number for ordering and loss detection (offset 9..11).
    pub sequence: u16,
    /// Timestamp in samples (offset 11..15, step 960 for 48kHz audio, 90kHz clock for video).
    pub timestamp: u32,
    /// Frame ID for video (offset 15..17). For audio equals sequence.
    pub frame_id: u16,
    /// Current fragment index within this frame (offset 17). For audio: 0.
    pub fragment_index: u8,
    /// Total number of fragments in this frame (offset 18). For audio: 1.
    pub total_fragments: u8,
    /// Flags bitmask (offset 19). Bit 0: silence, Bit 1: E2EE.
    pub flags: u8,
}

impl MediaHeader {
    /// Create a new header for an Opus audio datagram.
    pub fn new_audio(
        channel_id: u32,
        sender_peer_id: u32,
        sequence: u16,
        timestamp: u32,
        is_silence: bool,
        is_e2ee: bool,
    ) -> Self {
        let mut flags = 0u8;
        if is_silence {
            flags |= FLAG_SILENCE;
        }
        if is_e2ee {
            flags |= FLAG_E2EE;
        }

        Self {
            media_type: MediaType::Audio,
            channel_id,
            sender_peer_id,
            sequence,
            timestamp,
            frame_id: sequence,
            fragment_index: 0,
            total_fragments: 1,
            flags,
        }
    }

    /// Create a new header for a video keyframe (IDR).
    pub fn new_video_key(
        channel_id: u32,
        sender_peer_id: u32,
        sequence: u16,
        timestamp: u32,
        frame_id: u16,
        is_e2ee: bool,
    ) -> Self {
        let mut flags = 0u8;
        if is_e2ee {
            flags |= FLAG_E2EE;
        }

        Self {
            media_type: MediaType::VideoKey,
            channel_id,
            sender_peer_id,
            sequence,
            timestamp,
            frame_id,
            fragment_index: 0,
            total_fragments: 1,
            flags,
        }
    }

    /// Create a new header for a video delta (P) frame fragment.
    #[allow(clippy::too_many_arguments)]
    pub fn new_video_delta(
        channel_id: u32,
        sender_peer_id: u32,
        sequence: u16,
        timestamp: u32,
        frame_id: u16,
        fragment_index: u8,
        total_fragments: u8,
        is_e2ee: bool,
    ) -> Self {
        let mut flags = 0u8;
        if is_e2ee {
            flags |= FLAG_E2EE;
        }

        Self {
            media_type: MediaType::VideoDelta,
            channel_id,
            sender_peer_id,
            sequence,
            timestamp,
            frame_id,
            fragment_index,
            total_fragments,
            flags,
        }
    }

    /// Check if this packet is the last one before silence.
    pub fn is_silence(&self) -> bool {
        (self.flags & FLAG_SILENCE) != 0
    }

    /// Check if this packet payload is encrypted (E2EE).
    pub fn is_e2ee(&self) -> bool {
        (self.flags & FLAG_E2EE) != 0
    }

    /// Serialize this 20-byte header into a fixed-size byte array.
    pub fn to_bytes(&self) -> [u8; MEDIA_HEADER_SIZE] {
        let mut buf = [0u8; MEDIA_HEADER_SIZE];
        buf[0] = self.media_type as u8;
        buf[1..5].copy_from_slice(&self.channel_id.to_be_bytes());
        buf[5..9].copy_from_slice(&self.sender_peer_id.to_be_bytes());
        buf[9..11].copy_from_slice(&self.sequence.to_be_bytes());
        buf[11..15].copy_from_slice(&self.timestamp.to_be_bytes());
        buf[15..17].copy_from_slice(&self.frame_id.to_be_bytes());
        buf[17] = self.fragment_index;
        buf[18] = self.total_fragments;
        buf[19] = self.flags;
        buf
    }

    /// Write this header into a buffer implementing [`BufMut`].
    pub fn write_to(&self, buf: &mut impl BufMut) {
        buf.put_u8(self.media_type as u8);
        buf.put_u32(self.channel_id);
        buf.put_u32(self.sender_peer_id);
        buf.put_u16(self.sequence);
        buf.put_u32(self.timestamp);
        buf.put_u16(self.frame_id);
        buf.put_u8(self.fragment_index);
        buf.put_u8(self.total_fragments);
        buf.put_u8(self.flags);
    }

    /// Parse a header from a slice of at least 20 bytes.
    pub fn from_slice(slice: &[u8]) -> Result<Self, MediaHeaderError> {
        if slice.len() < MEDIA_HEADER_SIZE {
            return Err(MediaHeaderError::BufferTooShort {
                actual: slice.len(),
                expected: MEDIA_HEADER_SIZE,
            });
        }

        let media_type = MediaType::from_byte(slice[0])
            .ok_or(MediaHeaderError::InvalidMediaType(slice[0]))?;

        let channel_id = u32::from_be_bytes([slice[1], slice[2], slice[3], slice[4]]);
        let sender_peer_id = u32::from_be_bytes([slice[5], slice[6], slice[7], slice[8]]);
        let sequence = u16::from_be_bytes([slice[9], slice[10]]);
        let timestamp = u32::from_be_bytes([slice[11], slice[12], slice[13], slice[14]]);
        let frame_id = u16::from_be_bytes([slice[15], slice[16]]);
        let fragment_index = slice[17];
        let total_fragments = slice[18];
        let flags = slice[19];

        Ok(Self {
            media_type,
            channel_id,
            sender_peer_id,
            sequence,
            timestamp,
            frame_id,
            fragment_index,
            total_fragments,
            flags,
        })
    }

    /// Read and consume a header from a buffer implementing [`Buf`].
    pub fn read_from(buf: &mut impl Buf) -> Result<Self, MediaHeaderError> {
        if buf.remaining() < MEDIA_HEADER_SIZE {
            return Err(MediaHeaderError::BufferTooShort {
                actual: buf.remaining(),
                expected: MEDIA_HEADER_SIZE,
            });
        }

        let media_type_byte = buf.get_u8();
        let media_type = MediaType::from_byte(media_type_byte)
            .ok_or(MediaHeaderError::InvalidMediaType(media_type_byte))?;

        let channel_id = buf.get_u32();
        let sender_peer_id = buf.get_u32();
        let sequence = buf.get_u16();
        let timestamp = buf.get_u32();
        let frame_id = buf.get_u16();
        let fragment_index = buf.get_u8();
        let total_fragments = buf.get_u8();
        let flags = buf.get_u8();

        Ok(Self {
            media_type,
            channel_id,
            sender_peer_id,
            sequence,
            timestamp,
            frame_id,
            fragment_index,
            total_fragments,
            flags,
        })
    }

    /// Overwrite `sender_peer_id` directly in raw bytes at offset 5..9.
    /// Used by SFU to rewrite the authenticated sender ID without re-serializing.
    pub fn overwrite_sender_peer_id(raw_datagram: &mut [u8], auth_peer_id: u32) -> Result<(), MediaHeaderError> {
        if raw_datagram.len() < MEDIA_HEADER_SIZE {
            return Err(MediaHeaderError::BufferTooShort {
                actual: raw_datagram.len(),
                expected: MEDIA_HEADER_SIZE,
            });
        }
        raw_datagram[5..9].copy_from_slice(&auth_peer_id.to_be_bytes());
        Ok(())
    }

    /// Generate the 12-byte nonce for ChaCha20-Poly1305 E2EE encryption:
    /// `sender_peer_id` (4B) + `timestamp` (4B) + `sequence` (2B) + `frame_id` (2B).
    pub fn e2ee_nonce(&self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[0..4].copy_from_slice(&self.sender_peer_id.to_be_bytes());
        nonce[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        nonce[8..10].copy_from_slice(&self.sequence.to_be_bytes());
        nonce[10..12].copy_from_slice(&self.frame_id.to_be_bytes());
        nonce
    }
}

/// Errors that can occur when parsing a [`MediaHeader`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MediaHeaderError {
    #[error("Buffer too short: has {actual} bytes, expected at least {expected}")]
    BufferTooShort { actual: usize, expected: usize },

    #[error("Invalid media type byte: {0:#04x}")]
    InvalidMediaType(u8),
}

/// A parsed media packet: header + payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPacket {
    pub header: MediaHeader,
    pub payload: Bytes,
}

impl MediaPacket {
    /// Create a new media packet.
    pub fn new(header: MediaHeader, payload: Bytes) -> Self {
        Self { header, payload }
    }

    /// Encode the packet into a continuous buffer ready for QUIC transmission.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(MEDIA_HEADER_SIZE + self.payload.len());
        self.header.write_to(&mut buf);
        buf.extend_from_slice(&self.payload);
        buf.freeze()
    }

    /// Parse a complete media packet from raw datagram bytes.
    pub fn parse(mut data: Bytes) -> Result<Self, MediaHeaderError> {
        let header = MediaHeader::read_from(&mut data)?;
        Ok(Self {
            header,
            payload: data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_header_round_trip() {
        let header = MediaHeader {
            media_type: MediaType::Audio,
            channel_id: 42,
            sender_peer_id: 1001,
            sequence: 500,
            timestamp: 480_000,
            frame_id: 500,
            fragment_index: 0,
            total_fragments: 1,
            flags: FLAG_SILENCE | FLAG_E2EE,
        };

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 20);

        let parsed = MediaHeader::from_slice(&bytes).unwrap();
        assert_eq!(header, parsed);
        assert!(parsed.is_silence());
        assert!(parsed.is_e2ee());
    }

    #[test]
    fn test_video_delta_header() {
        let header = MediaHeader::new_video_delta(
            1, 2, 10, 90_000, 3, 2, 5, false,
        );
        assert_eq!(header.media_type, MediaType::VideoDelta);
        assert_eq!(header.frame_id, 3);
        assert_eq!(header.fragment_index, 2);
        assert_eq!(header.total_fragments, 5);
        assert!(!header.is_silence());
        assert!(!header.is_e2ee());

        let mut buf = BytesMut::new();
        header.write_to(&mut buf);
        assert_eq!(buf.len(), 20);

        let parsed = MediaHeader::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_overwrite_sender_peer_id() {
        let header = MediaHeader::new_audio(1, 0, 1, 960, false, false);
        let mut raw = header.to_bytes();

        MediaHeader::overwrite_sender_peer_id(&mut raw, 777).unwrap();
        let parsed = MediaHeader::from_slice(&raw).unwrap();
        assert_eq!(parsed.sender_peer_id, 777);
    }

    #[test]
    fn test_e2ee_nonce() {
        let header = MediaHeader {
            media_type: MediaType::Audio,
            channel_id: 1,
            sender_peer_id: 0x01020304,
            sequence: 0x0506,
            timestamp: 0x0708090A,
            frame_id: 0x0B0C,
            fragment_index: 0,
            total_fragments: 1,
            flags: FLAG_E2EE,
        };

        let nonce = header.e2ee_nonce();
        assert_eq!(nonce.len(), 12);
        assert_eq!(&nonce[0..4], &[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(&nonce[4..8], &[0x07, 0x08, 0x09, 0x0A]);
        assert_eq!(&nonce[8..10], &[0x05, 0x06]);
        assert_eq!(&nonce[10..12], &[0x0B, 0x0C]);
    }
}
