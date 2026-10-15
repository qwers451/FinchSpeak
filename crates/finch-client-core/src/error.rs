//! Client error type.

use finch_protocol::codec::CodecError;

/// Errors returned by the REST client and the QUIC session.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Server answered with a non-success HTTP status.
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },

    /// Request could not be sent or the response could not be read.
    #[error("Network error: {0}")]
    Network(String),

    /// QUIC connection or stream failure.
    #[error("QUIC error: {0}")]
    Quic(String),

    /// Control stream framing or MessagePack failure.
    #[error("Codec error: {0}")]
    Codec(#[from] CodecError),

    /// Server rejected the QUIC authentication.
    #[error("Authentication rejected ({code}): {message}")]
    AuthRejected { code: u16, message: String },

    /// Server sent a message that is not valid at this point of the protocol.
    #[error("Unexpected message: {0}")]
    UnexpectedMessage(String),

    /// First contact with a server whose fingerprint the user has not confirmed yet.
    #[error("Unknown server {hostname}: confirm certificate fingerprint {fingerprint}")]
    UntrustedServer { hostname: String, fingerprint: String },

    /// The server certificate differs from the pinned one.
    #[error("Certificate of {hostname} changed: expected {expected}, got {actual}")]
    FingerprintMismatch {
        hostname: String,
        expected: String,
        actual: String,
    },

    /// The session is closed.
    #[error("Session closed")]
    Closed,
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        Self::Network(e.to_string())
    }
}
