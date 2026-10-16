//! Data models shared between client and server.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A registered user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status: UserStatus,
    /// What they wrote next to their status ("In a meeting"), if anything.
    #[serde(default)]
    pub status_text: Option<String>,
    pub role: UserRole,
}

/// User role on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    Moderator,
    Member,
}

/// Online presence status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    Online,
    Idle,
    DoNotDisturb,
    Offline,
}

/// A channel (text or voice).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Channel {
    pub id: Uuid,
    /// Short 32-bit ID used in media packet headers.
    pub short_id: u32,
    pub name: String,
    pub channel_type: ChannelType,
    pub topic: Option<String>,
    pub position: i32,
    pub bitrate_kbps: u32,
}

/// Channel type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Text,
    Voice,
}

/// Why a screen share ended, as viewers are told.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShareEndReason {
    /// The sharer pressed stop.
    #[default]
    Stopped,
    /// The sharer left the voice room (or went to another one).
    Left,
    /// The sharer's connection dropped.
    Disconnected,
    /// The voice room was deleted.
    RoomClosed,
}

/// Video codec for screen sharing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
}

/// Info about an uploaded file attachment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentInfo {
    pub id: Uuid,
    pub filename: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub url: String,
    /// Pixel size of an image attachment.
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    /// A 320 px preview exists at `{url}/thumb`.
    #[serde(default)]
    pub has_thumb: bool,
}

/// The message a reply answers, in the shape the client shows above the reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyRef {
    pub message_id: Uuid,
    pub author_name: String,
    /// The first 120 characters of the original.
    pub snippet: String,
    /// The original was deleted; `author_name` and `snippet` are empty then.
    #[serde(default)]
    pub deleted: bool,
}

/// Unread state of one channel for the signed-in user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelUnread {
    pub channel_id: Uuid,
    pub count: u32,
    /// Of those, how many mention the user.
    pub mentions: u32,
    /// The oldest unread message: where the "New messages" divider goes.
    pub first_unread_id: Option<Uuid>,
}

/// A chat message from history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryMessage {
    pub id: Uuid,
    pub author_id: Uuid,
    pub author_name: String,
    pub content: String,
    pub attachments: Vec<AttachmentInfo>,
    pub timestamp: i64,
    pub edited_at: Option<i64>,
    /// The id the author gave this message when sending it, if any.
    #[serde(default)]
    pub client_msg_id: Option<Uuid>,
    /// Emoji reactions, one entry per emoji.
    #[serde(default)]
    pub reactions: Vec<ReactionInfo>,
    #[serde(default)]
    pub reply_to: Option<ReplyRef>,
    /// Users the text mentions with `@login`.
    #[serde(default)]
    pub mentions: Vec<Uuid>,
}

/// Who reacted to a message with one emoji.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReactionInfo {
    pub emoji: String,
    pub user_ids: Vec<Uuid>,
}

/// Voice state for a user in a voice channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceState {
    pub user_id: Uuid,
    pub peer_id: u32,
    pub username: String,
    pub channel_id: Uuid,
    pub muted: bool,
    pub deafened: bool,
    pub sharing_screen: bool,
    /// A moderator silenced this person in the room.
    #[serde(default)]
    pub server_muted: bool,
}

/// Mapping between channel UUID and compact short_id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelMapping {
    pub id: Uuid,
    pub short_id: u32,
}

/// Full server state sent to the client upon authentication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerState {
    /// All channels on the server.
    pub channels: Vec<Channel>,
    /// Currently online users.
    pub online_users: Vec<User>,
    /// Users currently in voice channels.
    pub voice_states: Vec<VoiceState>,
    /// Unread messages per channel for the user who asks (empty when read or unknown).
    #[serde(default)]
    pub unread: Vec<ChannelUnread>,
}
