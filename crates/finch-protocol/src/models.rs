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
    pub role: UserRole,
}

/// User role on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
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
}
