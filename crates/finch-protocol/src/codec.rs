//! Codec utilities for framing and serializing Control Stream messages.
//!
//! Messages on the QUIC bi-directional Control Stream are length-prefixed:
//!
//! ```text
//! [4 bytes big-endian length][N bytes MessagePack payload]
//! ```
//!
//! This module provides helpers to encode/decode messages in that format.

use bytes::{Buf, BufMut, BytesMut};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

/// Maximum allowed message size (1 MB) to prevent memory exhaustion.
const MAX_MESSAGE_SIZE: u32 = 1024 * 1024;

/// Errors that can occur during message encoding or decoding.
#[derive(Debug, Error)]
pub enum CodecError {
    #[error("message too large: {0} bytes (max {MAX_MESSAGE_SIZE})")]
    MessageTooLarge(u32),

    #[error("incomplete frame: need {needed} more bytes")]
    Incomplete { needed: usize },

    #[error("serialization error: {0}")]
    Serialize(#[from] rmp_serde::encode::Error),

    #[error("deserialization error: {0}")]
    Deserialize(#[from] rmp_serde::decode::Error),
}

/// Encode a message into a length-prefixed MessagePack frame.
///
/// Returns the complete frame as a `Vec<u8>`:
/// `[4 bytes big-endian length][N bytes MessagePack payload]`
pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>, CodecError> {
    let payload = rmp_serde::to_vec_named(msg)?;
    let len = payload.len() as u32;

    if len > MAX_MESSAGE_SIZE {
        return Err(CodecError::MessageTooLarge(len));
    }

    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.put_u32(len);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Try to decode a length-prefixed MessagePack message from a buffer.
///
/// On success, the consumed bytes are removed from `buf` and the decoded
/// message is returned. If the buffer doesn't contain a complete frame,
/// returns [`CodecError::Incomplete`].
pub fn decode_message<T: DeserializeOwned>(buf: &mut BytesMut) -> Result<T, CodecError> {
    if buf.len() < 4 {
        return Err(CodecError::Incomplete { needed: 4 - buf.len() });
    }

    // Peek at the length without advancing
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);

    if len > MAX_MESSAGE_SIZE {
        return Err(CodecError::MessageTooLarge(len));
    }

    let total = 4 + len as usize;
    if buf.len() < total {
        return Err(CodecError::Incomplete {
            needed: total - buf.len(),
        });
    }

    // Skip the 4-byte length prefix
    buf.advance(4);

    // Take exactly `len` bytes for the payload
    let payload = buf.split_to(len as usize);
    let msg = rmp_serde::from_slice(&payload)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::ClientMessage;

    #[test]
    fn round_trip_client_message() {
        let msg = ClientMessage::Ping { timestamp: 42 };
        let frame = encode_message(&msg).unwrap();

        let mut buf = BytesMut::from(&frame[..]);
        let decoded: ClientMessage = decode_message(&mut buf).unwrap();

        match decoded {
            ClientMessage::Ping { timestamp } => assert_eq!(timestamp, 42),
            _ => panic!("unexpected message type"),
        }

        assert!(buf.is_empty(), "buffer should be fully consumed");
    }

    #[test]
    fn incomplete_frame_returns_error() {
        let msg = ClientMessage::LeaveVoice;
        let frame = encode_message(&msg).unwrap();

        // Feed only part of the frame
        let mut buf = BytesMut::from(&frame[..3]);
        let result: Result<ClientMessage, _> = decode_message(&mut buf);
        assert!(matches!(result, Err(CodecError::Incomplete { .. })));
    }

    #[test]
    fn multiple_messages_in_buffer() {
        let msg1 = ClientMessage::Ping { timestamp: 1 };
        let msg2 = ClientMessage::Ping { timestamp: 2 };

        let frame1 = encode_message(&msg1).unwrap();
        let frame2 = encode_message(&msg2).unwrap();

        let mut buf = BytesMut::new();
        buf.extend_from_slice(&frame1);
        buf.extend_from_slice(&frame2);

        let decoded1: ClientMessage = decode_message(&mut buf).unwrap();
        let decoded2: ClientMessage = decode_message(&mut buf).unwrap();

        match (decoded1, decoded2) {
            (ClientMessage::Ping { timestamp: t1 }, ClientMessage::Ping { timestamp: t2 }) => {
                assert_eq!(t1, 1);
                assert_eq!(t2, 2);
            }
            _ => panic!("unexpected message types"),
        }
    }
}
