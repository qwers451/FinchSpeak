//! Client ↔ Server message types for the QUIC Control Stream.
//!
//! Messages are serialized with MessagePack and framed as:
//! `[4 bytes big-endian length][N bytes MessagePack payload]`

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{
    AttachmentInfo, Channel, ChannelMapping, HistoryMessage, ReplyRef, ServerState, UserStatus, VideoCodec,
};

// ─── Client → Server ────────────────────────────────────────────────────────

/// Messages sent from the client to the server on the Control Stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ClientMessage {
    /// First message after QUIC handshake — authenticate with a JWT token.
    Authenticate {
        token: String,
    },

    /// Join a voice channel. The client can only be in one voice channel at a time.
    JoinVoice {
        channel_id: Uuid,
    },

    /// Leave the current voice channel.
    LeaveVoice,

    /// The user is typing in a channel (for the "is typing" hint; clients send it at most every few seconds).
    Typing {
        channel_id: Uuid,
    },

    /// Update own voice state (mute/deafen).
    UpdateVoiceState {
        muted: bool,
        deafened: bool,
    },

    /// Send a text message to a channel.
    SendMessage {
        channel_id: Uuid,
        content: String,
        /// IDs of previously uploaded attachments to include.
        #[serde(default)]
        attachments: Vec<Uuid>,
        /// Chosen by the sender. Echoed in [`ServerMessage::NewMessage`] and
        /// [`ServerMessage::MessageRejected`], and makes a retry idempotent: a message with an
        /// id the sender already used is not stored or broadcast again.
        #[serde(default)]
        client_msg_id: Option<Uuid>,
        /// The message this one answers (same channel).
        #[serde(default)]
        reply_to: Option<Uuid>,
    },

    /// Request chat message history for a channel.
    FetchHistory {
        channel_id: Uuid,
        /// Fetch messages created before this unix timestamp (None = latest).
        before: Option<i64>,
        /// Maximum number of messages to return.
        limit: u32,
    },

    /// Start screen sharing in the current voice channel.
    StartScreenShare {
        codec: VideoCodec,
        width: u32,
        height: u32,
        fps: u8,
    },

    /// Stop screen sharing.
    StopScreenShare,

    /// Subscribe to a user's screen share stream.
    SubscribeScreenShare {
        target_user_id: Uuid,
    },

    /// Unsubscribe from a user's screen share stream.
    UnsubscribeScreenShare {
        target_user_id: Uuid,
    },

    /// Request an IDR keyframe from a streamer when delta frames are lost.
    RequestKeyframe {
        target_peer_id: u32,
    },

    /// Heartbeat ping. Server responds with [`ServerMessage::Pong`].
    Ping {
        timestamp: u64,
    },
}

// ─── Server → Client ────────────────────────────────────────────────────────

/// Messages sent from the server to the client on the Control Stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Authentication succeeded. Contains session peer ID, channel mappings, and initial server state.
    Authenticated {
        user_id: Uuid,
        session_peer_id: u32,
        channels: Vec<ChannelMapping>,
        initial_state: ServerState,
    },

    /// An error occurred.
    Error {
        code: u16,
        message: String,
    },

    /// A new text message was posted in a channel (broadcast to all clients).
    NewMessage {
        message_id: Uuid,
        channel_id: Uuid,
        author_id: Uuid,
        author_name: String,
        content: String,
        #[serde(default)]
        attachments: Vec<AttachmentInfo>,
        timestamp: i64,
        /// The id the author gave this message, if any.
        #[serde(default)]
        client_msg_id: Option<Uuid>,
        #[serde(default)]
        reply_to: Option<ReplyRef>,
        /// Users the text mentions with `@login`, so clients can notify without opening the channel.
        #[serde(default)]
        mentions: Vec<Uuid>,
    },

    /// A [`ClientMessage::SendMessage`] that carried a `client_msg_id` was refused. Without an
    /// id the sender gets a plain [`ServerMessage::Error`].
    MessageRejected {
        client_msg_id: Uuid,
        code: u16,
        message: String,
    },

    /// Response to [`ClientMessage::FetchHistory`].
    MessageHistory {
        channel_id: Uuid,
        messages: Vec<HistoryMessage>,
        has_more: bool,
    },

    /// A user joined a voice channel.
    UserJoinedVoice {
        user_id: Uuid,
        peer_id: u32,
        username: String,
        channel_id: Uuid,
    },

    /// A user left a voice channel.
    UserLeftVoice {
        user_id: Uuid,
        peer_id: u32,
        channel_id: Uuid,
    },

    /// Voice state update for a user (mute/deafen changed).
    UserVoiceStateChanged {
        peer_id: u32,
        muted: bool,
        deafened: bool,
    },

    /// A user started sharing their screen.
    ScreenShareStarted {
        user_id: Uuid,
        peer_id: u32,
        codec: VideoCodec,
        width: u32,
        height: u32,
        fps: u8,
    },

    /// A user stopped sharing their screen.
    ScreenShareStopped {
        peer_id: u32,
    },

    /// Instruct the streamer to immediately generate and send an IDR keyframe.
    KeyframeRequested,

    /// A channel was created (broadcast to all clients).
    ChannelCreated {
        channel: Channel,
    },

    /// A user changed their display name.
    UserProfileChanged {
        user_id: Uuid,
        display_name: String,
    },

    /// Somebody is typing in a channel.
    UserTyping {
        channel_id: Uuid,
        user_id: Uuid,
        username: String,
    },

    /// A message was edited by its author.
    MessageEdited {
        message_id: Uuid,
        channel_id: Uuid,
        content: String,
        edited_at: i64,
    },

    /// Somebody added or removed an emoji reaction.
    ReactionChanged {
        message_id: Uuid,
        channel_id: Uuid,
        user_id: Uuid,
        emoji: String,
        added: bool,
    },

    /// A message was deleted (by its author or an admin).
    MessageDeleted {
        message_id: Uuid,
        channel_id: Uuid,
    },

    /// A channel was renamed, got a new topic or position.
    ChannelUpdated {
        channel: Channel,
    },

    /// A channel was deleted; people in it have already been sent out.
    ChannelDeleted {
        channel_id: Uuid,
    },

    /// A user's online presence changed.
    PresenceUpdate {
        user_id: Uuid,
        status: UserStatus,
    },

    /// Heartbeat pong — echo of the client's timestamp.
    Pong {
        timestamp: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_roundtrip() {
        let msg = ClientMessage::StartScreenShare {
            codec: VideoCodec::H264,
            width: 1920,
            height: 1080,
            fps: 60,
        };

        let encoded = rmp_serde::to_vec(&msg).unwrap();
        let decoded: ClientMessage = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_server_message_roundtrip() {
        let msg = ServerMessage::KeyframeRequested;
        let encoded = rmp_serde::to_vec(&msg).unwrap();
        let decoded: ServerMessage = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }
}
