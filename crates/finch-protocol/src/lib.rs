//! # FinchSpeak Protocol
//!
//! Shared types, message definitions, and serialization for the FinchSpeak protocol.
//!
//! This crate defines the wire format for communication between
//! the FinchSpeak client and server over QUIC.

pub mod codec;
pub mod media;
pub mod messages;
pub mod models;

pub use codec::{decode_message, encode_message};
pub use media::{MediaHeader, MediaType, MAX_DATAGRAM_PAYLOAD, MEDIA_HEADER_SIZE};
pub use messages::{ClientMessage, ServerMessage};
