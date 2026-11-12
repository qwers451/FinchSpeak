//! REST API client for authentication, channels, message history and files.

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use finch_protocol::models::{AttachmentInfo, HistoryMessage, MusicTrack, ServerState, SoundInfo};
use reqwest::multipart::{Form, Part};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::error::ClientError;
use crate::tofu::{TofuIssue, TofuIssueSlot, TofuVerifier};

/// Music files are large; the client-wide 30 s timeout is too short for them on slow links.
const MUSIC_TRANSFER_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDto {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserDto,
    /// QUIC port announced by the server (older servers do not send it).
    #[serde(default)]
    pub quic_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfoDto {
    pub id: String,
    pub short_id: u32,
    pub name: String,
    pub channel_type: String,
    pub topic: Option<String>,
    pub position: i32,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesHistoryDto {
    pub channel_id: String,
    pub messages: Vec<HistoryMessage>,
    pub has_more: bool,
}

/// A member as listed to admins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUserDto {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
    pub banned: bool,
    #[serde(default)]
    pub banned_until: Option<i64>,
    pub online: bool,
    #[serde(default)]
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHitDto {
    pub message_id: String,
    pub channel_id: String,
    pub channel_name: String,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntryDto {
    pub id: i64,
    pub at: i64,
    pub actor_name: String,
    pub action: String,
    pub target: Option<String>,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerOverviewDto {
    pub version: String,
    pub uptime_secs: u64,
    pub certificate_fingerprint: Option<String>,
    pub quic_port: u16,
    pub http_port: u16,
    pub registration: String,
    pub backups_count: usize,
    pub last_backup_at: Option<i64>,
    pub last_backup_bytes: Option<u64>,
    pub backup_dir: String,
    pub uploads_bytes: u64,
    pub log_dir: String,
}

/// One measurement of the server's health (see `GET /api/admin/health`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSampleDto {
    pub at: i64,
    pub cpu_percent: f32,
    pub rss_bytes: u64,
    pub system_used_bytes: u64,
    pub system_total_bytes: u64,
    pub container_limit_bytes: Option<u64>,
    pub disk_free_bytes: u64,
    pub sessions: u32,
    pub voice_users: u32,
    pub bytes_in_per_sec: u64,
    pub bytes_out_per_sec: u64,
    pub datagrams_dropped: u64,
}

/// The server's health panel: the newest sample, the last hour and the folder sizes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReportDto {
    pub now: HealthSampleDto,
    pub history: Vec<HealthSampleDto>,
    pub db_bytes: u64,
    pub uploads_bytes: u64,
    pub music_bytes: u64,
    pub uptime_secs: u64,
    /// Codes such as `memory-high` and `disk-low`.
    pub warnings: Vec<String>,
}

/// An invitation code as listed by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteDto {
    pub code: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub max_uses: i64,
    pub uses: i64,
}

/// One signed-in device as listed by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDto {
    pub id: String,
    pub device_name: Option<String>,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub current: bool,
}

/// HTTP client bound to one FinchSpeak server.
#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
    token: Option<String>,
    trust_issue: TofuIssueSlot,
    hostname: String,
    device_name: Option<String>,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Result<Self, ClientError> {
        Self::build(base_url, None)
    }

    /// Use the same explicitly confirmed certificate pins as the QUIC connection.
    pub fn with_tofu(base_url: &str, pins: Arc<dashmap::DashMap<String, String>>) -> Result<Self, ClientError> {
        Self::build(base_url, Some(pins))
    }

    fn build(base_url: &str, pins: Option<Arc<dashmap::DashMap<String, String>>>) -> Result<Self, ClientError> {
        let url = reqwest::Url::parse(base_url).map_err(|e| ClientError::Network(e.to_string()))?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
            || url.query().is_some() || url.fragment().is_some() || url.path() != "/" {
            return Err(ClientError::Network("Server URL must be an HTTPS origin (https://host:port); migrate old http:// addresses explicitly".into()));
        }
        let hostname = url.host_str().ok_or_else(|| ClientError::Network("Missing server hostname".into()))?.to_string();
        let trust_issue = Arc::new(Mutex::new(None));
        let mut builder = reqwest::Client::builder().https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30));
        if let Some(pins) = pins {
            let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions().map_err(|e| ClientError::Network(e.to_string()))?
                .dangerous().with_custom_certificate_verifier(Arc::new(TofuVerifier::with_policy(
                    hostname.clone(), pins, true, trust_issue.clone(),
                ))).with_no_client_auth();
            builder = builder.use_preconfigured_tls(tls);
        }
        let http = builder.build()?;
        Ok(Self {
            base_url: url.as_str().trim_end_matches('/').to_string(),
            http,
            token: None,
            trust_issue,
            hostname,
            device_name: None,
        })
    }

    /// Complete certificate verification without transmitting credentials or a bearer token.
    pub async fn verify_trust(&self) -> Result<(), ClientError> {
        self.send(self.http.get(self.url("/api/state"))).await?;
        Ok(())
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response, ClientError> {
        request.send().await.map_err(|error| {
            match self.trust_issue.lock().ok().and_then(|mut slot| slot.take()) {
                Some(TofuIssue::Unknown { fingerprint }) => ClientError::UntrustedServer { hostname: self.hostname.clone(), fingerprint },
                Some(TofuIssue::Mismatch { expected, actual }) => ClientError::FingerprintMismatch { hostname: self.hostname.clone(), expected, actual },
                None => error.into(),
            }
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Name this computer shows under in the "Devices" list; sent with login and registration.
    pub fn set_device_name(&mut self, name: Option<String>) {
        self.device_name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    }

    pub async fn register(
        &self,
        username: &str,
        password: &str,
        display_name: &str,
    ) -> Result<AuthResponse, ClientError> {
        self.register_inner(username, password, display_name, None).await
    }

    /// Register with an invitation code (needed on servers that do not allow open registration).
    pub async fn register_with_invite(
        &self,
        username: &str,
        password: &str,
        display_name: &str,
        invite_code: &str,
    ) -> Result<AuthResponse, ClientError> {
        self.register_inner(username, password, display_name, Some(invite_code)).await
    }

    async fn register_inner(
        &self,
        username: &str,
        password: &str,
        display_name: &str,
        invite_code: Option<&str>,
    ) -> Result<AuthResponse, ClientError> {
        let req = self
            .http
            .post(self.url("/api/auth/register"))
            .json(&serde_json::json!({
                "username": username,
                "display_name": display_name,
                "password": password,
                "invite_code": invite_code,
                "device_name": self.device_name
            }));
        parse_json(self.send(req).await?).await
    }

    /// Admin: rename a channel and/or change its topic.
    pub async fn update_channel(
        &self,
        channel_id: Uuid,
        name: Option<&str>,
        topic: Option<&str>,
    ) -> Result<ChannelInfoDto, ClientError> {
        let req = self
            .authorized(self.http.patch(self.url(&format!("/api/channels/{channel_id}"))))
            .json(&serde_json::json!({ "name": name, "topic": topic }));
        parse_json(self.send(req).await?).await
    }

    /// Admin: delete a channel with its messages (people in a voice channel are sent out).
    pub async fn delete_channel(&self, channel_id: Uuid) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url(&format!("/api/channels/{channel_id}"))));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Admin: set the order of all channels.
    pub async fn reorder_channels(&self, channel_ids: &[Uuid]) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.put(self.url("/api/channels/order")))
            .json(&serde_json::json!({ "channel_ids": channel_ids }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Change your display name; returns the updated profile.
    pub async fn update_profile(&self, display_name: &str) -> Result<UserDto, ClientError> {
        let req = self
            .authorized(self.http.patch(self.url("/api/auth/me")))
            .json(&serde_json::json!({ "display_name": display_name }));
        parse_json(self.send(req).await?).await
    }

    /// Change your password; other devices are signed out.
    pub async fn change_password(&self, current_password: &str, new_password: &str) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.post(self.url("/api/auth/password")))
            .json(&serde_json::json!({ "current_password": current_password, "new_password": new_password }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Edit one of your messages.
    pub async fn edit_message(&self, message_id: Uuid, content: &str) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.patch(self.url(&format!("/api/messages/{message_id}"))))
            .json(&serde_json::json!({ "content": content }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// React to a message with an emoji.
    pub async fn add_reaction(&self, message_id: Uuid, emoji: &str) -> Result<(), ClientError> {
        let req = self.authorized(self.http.put(self.reaction_url(message_id, emoji)));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Take your reaction back.
    pub async fn remove_reaction(&self, message_id: Uuid, emoji: &str) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.reaction_url(message_id, emoji)));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    fn reaction_url(&self, message_id: Uuid, emoji: &str) -> String {
        // The emoji is one path segment: percent-encode everything that is not unreserved.
        let encoded: String = emoji
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect();
        self.url(&format!("/api/messages/{message_id}/reactions/{encoded}"))
    }

    /// Delete your message (admins: anyone's).
    pub async fn delete_message(&self, message_id: Uuid) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url(&format!("/api/messages/{message_id}"))));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Admin: all members with role, ban and online state.
    pub async fn admin_users(&self) -> Result<Vec<AdminUserDto>, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/admin/users")));
        parse_json(self.send(req).await?).await
    }

    /// Admin: make a user an `"admin"` or a `"member"`.
    pub async fn set_user_role(&self, user_id: Uuid, role: &str) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.put(self.url(&format!("/api/admin/users/{user_id}/role"))))
            .json(&serde_json::json!({ "role": role }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Admin: disconnect a user and revoke their tokens (they may log in again).
    pub async fn kick_user(&self, user_id: Uuid) -> Result<(), ClientError> {
        self.admin_action(user_id, "kick").await
    }

    /// Admin: disconnect a user and refuse their logins.
    pub async fn ban_user(&self, user_id: Uuid) -> Result<(), ClientError> {
        self.admin_action(user_id, "ban").await
    }

    /// Ban for `duration_secs` seconds (`None`: for good).
    pub async fn ban_user_for(&self, user_id: Uuid, duration_secs: Option<i64>) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.post(self.url(&format!("/api/admin/users/{user_id}/ban"))))
            .json(&serde_json::json!({ "duration_secs": duration_secs }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Search messages (newest matches first), optionally in one channel.
    pub async fn search_messages(&self, text: &str, channel: Option<Uuid>, limit: u32, offset: u32) -> Result<Vec<SearchHitDto>, ClientError> {
        let mut req = self
            .authorized(self.http.get(self.url("/api/search")))
            .query(&[("q", text), ("limit", &limit.to_string()), ("offset", &offset.to_string())]);
        if let Some(channel) = channel {
            req = req.query(&[("channel", channel.to_string())]);
        }
        parse_json(self.send(req).await?).await
    }

    /// Moderator: `"kick"` out of the voice room, `"mute"` or `"unmute"` in it.
    pub async fn moderate_voice(&self, user_id: Uuid, action: &str) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.post(self.url(&format!("/api/admin/users/{user_id}/voice"))))
            .json(&serde_json::json!({ "action": action }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Moderator: move someone who is in a voice room into another voice channel.
    pub async fn move_voice(&self, user_id: Uuid, peer_id: u32, channel_id: Uuid) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.post(self.url(&format!("/api/admin/users/{user_id}/voice"))))
            .json(&serde_json::json!({ "action": "move", "peer_id": peer_id, "channel_id": channel_id }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// The log of administrative actions, newest first.
    pub async fn audit_log(&self, before: Option<i64>, limit: u32) -> Result<Vec<AuditEntryDto>, ClientError> {
        let mut path = format!("/api/admin/audit?limit={limit}");
        if let Some(before) = before {
            path.push_str(&format!("&before={before}"));
        }
        parse_json(self.send(self.authorized(self.http.get(self.url(&path)))).await?).await
    }

    pub async fn server_overview(&self) -> Result<ServerOverviewDto, ClientError> {
        parse_json(self.send(self.authorized(self.http.get(self.url("/api/admin/server")))).await?).await
    }

    /// Admin: CPU, memory, disk, traffic and the last hour of samples.
    pub async fn server_health(&self) -> Result<HealthReportDto, ClientError> {
        parse_json(self.send(self.authorized(self.http.get(self.url("/api/admin/health")))).await?).await
    }

    pub async fn set_registration(&self, mode: &str) -> Result<(), ClientError> {
        let req = self.authorized(self.http.put(self.url("/api/admin/registration"))).json(&serde_json::json!({ "mode": mode }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    pub async fn backup_now(&self) -> Result<(), ClientError> {
        check_status(self.send(self.authorized(self.http.post(self.url("/api/admin/backup")))).await?).await?;
        Ok(())
    }

    pub async fn unban_user(&self, user_id: Uuid) -> Result<(), ClientError> {
        self.admin_action(user_id, "unban").await
    }

    /// Admin: give a member a new temporary password (returned once); they are signed out everywhere.
    pub async fn reset_password(&self, user_id: Uuid) -> Result<String, ClientError> {
        #[derive(Deserialize)]
        struct TemporaryPasswordReply {
            temporary_password: String,
        }
        let req = self.authorized(self.http.post(self.url(&format!("/api/admin/users/{user_id}/password-reset"))));
        let reply: TemporaryPasswordReply = parse_json(self.send(req).await?).await?;
        Ok(reply.temporary_password)
    }

    async fn admin_action(&self, user_id: Uuid, action: &str) -> Result<(), ClientError> {
        let req = self.authorized(self.http.post(self.url(&format!("/api/admin/users/{user_id}/{action}"))));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Admin: create an invitation code.
    pub async fn create_invite(&self, max_uses: Option<i64>, expires_hours: Option<i64>) -> Result<InviteDto, ClientError> {
        let req = self
            .authorized(self.http.post(self.url("/api/invites")))
            .json(&serde_json::json!({ "max_uses": max_uses, "expires_hours": expires_hours }));
        parse_json(self.send(req).await?).await
    }

    /// Admin: all invitation codes.
    pub async fn list_invites(&self) -> Result<Vec<InviteDto>, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/invites")));
        parse_json(self.send(req).await?).await
    }

    /// Admin: make a code unusable.
    pub async fn revoke_invite(&self, code: &str) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url(&format!("/api/invites/{code}"))));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<AuthResponse, ClientError> {
        let req = self
            .http
            .post(self.url("/api/auth/login"))
            .json(&serde_json::json!({
                "username": username,
                "password": password,
                "device_name": self.device_name
            }));
        parse_json(self.send(req).await?).await
    }

    /// Your signed-in devices.
    pub async fn list_sessions(&self) -> Result<Vec<SessionDto>, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/auth/sessions")));
        parse_json(self.send(req).await?).await
    }

    /// Sign one device out.
    pub async fn end_session(&self, session_id: &str) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url(&format!("/api/auth/sessions/{session_id}"))));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Sign out every device but this one.
    pub async fn end_other_sessions(&self) -> Result<(), ClientError> {
        let req = self.authorized(self.http.post(self.url("/api/auth/sessions/revoke-others")));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    pub async fn me(&self) -> Result<UserDto, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/auth/me")));
        parse_json(self.send(req).await?).await
    }

    pub async fn refresh(&self) -> Result<AuthResponse, ClientError> {
        let req = self.authorized(self.http.post(self.url("/api/auth/refresh")));
        parse_json(self.send(req).await?).await
    }

    pub async fn logout(&self) -> Result<(), ClientError> {
        let req = self.authorized(self.http.post(self.url("/api/auth/logout")));
        let response = self.send(req).await?;
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            check_status(response).await?;
        }
        Ok(())
    }

    pub async fn server_state(&self) -> Result<ServerState, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/state")));
        parse_json(self.send(req).await?).await
    }

    pub async fn list_channels(&self) -> Result<Vec<ChannelInfoDto>, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/channels")));
        parse_json(self.send(req).await?).await
    }

    pub async fn create_channel(
        &self,
        name: &str,
        channel_type: &str,
        topic: Option<String>,
        bitrate_kbps: Option<u32>,
    ) -> Result<ChannelInfoDto, ClientError> {
        let req = self
            .authorized(self.http.post(self.url("/api/channels")))
            .json(&serde_json::json!({
                "name": name,
                "channel_type": channel_type,
                "topic": topic,
                "bitrate_kbps": bitrate_kbps
            }));
        parse_json(self.send(req).await?).await
    }

    pub async fn fetch_messages(
        &self,
        channel_id: Uuid,
        before: Option<i64>,
        limit: u32,
    ) -> Result<MessagesHistoryDto, ClientError> {
        let mut path = format!("/api/channels/{channel_id}/messages?limit={limit}");
        if let Some(b) = before {
            path.push_str(&format!("&before={b}"));
        }
        let req = self.authorized(self.http.get(self.url(&path)));
        parse_json(self.send(req).await?).await
    }

    /// Page backwards through history: messages older than `before_id` (newest page when `None`).
    pub async fn fetch_messages_page(
        &self,
        channel_id: Uuid,
        before_id: Option<Uuid>,
        limit: u32,
    ) -> Result<MessagesHistoryDto, ClientError> {
        let mut path = format!("/api/channels/{channel_id}/messages?limit={limit}");
        if let Some(id) = before_id {
            path.push_str(&format!("&before_id={id}"));
        }
        let req = self.authorized(self.http.get(self.url(&path)));
        parse_json(self.send(req).await?).await
    }

    pub async fn upload_file(
        &self,
        filename: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<AttachmentInfo, ClientError> {
        let part = Part::bytes(data)
            .file_name(filename.to_string())
            .mime_str(content_type)?;
        let form = Form::new().part("file", part);
        let req = self
            .authorized(self.http.post(self.url("/api/files/upload")))
            .multipart(form);
        parse_json(self.send(req).await?).await
    }

    /// Uploads a voice message: an Ogg/Opus file with its length in milliseconds and its
    /// 64-value waveform (sent as hex text, 128 characters).
    pub async fn upload_voice_note(&self, data: Vec<u8>, duration_ms: u64, waveform: &[u8]) -> Result<AttachmentInfo, ClientError> {
        let part = Part::bytes(data).file_name("voice-message.ogg".to_string()).mime_str("audio/ogg")?;
        let hex: String = waveform.iter().map(|value| format!("{value:02x}")).collect();
        // The server reads these text fields before the file, so they go first.
        let form = Form::new()
            .text("duration_ms", duration_ms.to_string())
            .text("waveform", hex)
            .part("file", part);
        let req = self
            .authorized(self.http.post(self.url("/api/files/upload")))
            .multipart(form);
        parse_json(self.send(req).await?).await
    }

    /// Set your profile picture (any PNG/JPEG/WebP up to 2 MB; the server makes it 128x128).
    pub async fn upload_avatar(&self, data: Vec<u8>, content_type: &str) -> Result<UserDto, ClientError> {
        let part = Part::bytes(data).file_name("avatar".to_string()).mime_str(content_type)?;
        let req = self.authorized(self.http.post(self.url("/api/auth/avatar"))).multipart(Form::new().part("file", part));
        parse_json(self.send(req).await?).await
    }

    pub async fn delete_avatar(&self) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url("/api/auth/avatar")));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    pub async fn download_avatar(&self, user_id: Uuid) -> Result<Bytes, ClientError> {
        let req = self.authorized(self.http.get(self.url(&format!("/api/users/{user_id}/avatar"))));
        Ok(check_status(self.send(req).await?).await?.bytes().await?)
    }

    /// Like `download_avatar`, but answers `(None, etag)` when `etag` is still current.
    pub async fn download_avatar_cached(&self, user_id: Uuid, etag: Option<&str>) -> Result<(Option<Bytes>, Option<String>), ClientError> {
        let mut req = self.authorized(self.http.get(self.url(&format!("/api/users/{user_id}/avatar"))));
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let resp = self.send(req).await?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok((None, etag.map(str::to_string)));
        }
        let resp = check_status(resp).await?;
        let tag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
        Ok((Some(resp.bytes().await?), tag))
    }

    /// Set your join sound (a WAV of at most 2 seconds).
    pub async fn upload_join_sound(&self, data: Vec<u8>) -> Result<(), ClientError> {
        let part = Part::bytes(data).file_name("join-sound.wav".to_string()).mime_str("audio/wav")?;
        let req = self.authorized(self.http.post(self.url("/api/auth/join-sound"))).multipart(Form::new().part("file", part));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    pub async fn delete_join_sound(&self) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url("/api/auth/join-sound")));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// A user's join sound; `(None, etag)` when `etag` is still current. A 404 error means they have none.
    pub async fn download_join_sound_cached(&self, user_id: Uuid, etag: Option<&str>) -> Result<(Option<Bytes>, Option<String>), ClientError> {
        let mut req = self.authorized(self.http.get(self.url(&format!("/api/users/{user_id}/join-sound"))));
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let resp = self.send(req).await?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok((None, etag.map(str::to_string)));
        }
        let resp = check_status(resp).await?;
        let tag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
        Ok((Some(resp.bytes().await?), tag))
    }

    /// Tell the server how far you have read `channel_id` (only moves forward).
    pub async fn mark_read(&self, channel_id: Uuid, message_id: Uuid) -> Result<(), ClientError> {
        let req = self
            .authorized(self.http.post(self.url(&format!("/api/channels/{channel_id}/read"))))
            .json(&serde_json::json!({ "message_id": message_id }));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// The 320 px preview of an image attachment.
    pub async fn download_thumb(&self, id: Uuid) -> Result<Bytes, ClientError> {
        let req = self.authorized(self.http.get(self.url(&format!("/api/files/{id}/thumb"))));
        let resp = check_status(self.send(req).await?).await?;
        Ok(resp.bytes().await?)
    }

    pub async fn download_file(&self, id: Uuid) -> Result<Bytes, ClientError> {
        let req = self.authorized(self.http.get(self.url(&format!("/api/files/{id}"))));
        let resp = check_status(self.send(req).await?).await?;
        Ok(resp.bytes().await?)
    }

    /// The soundboard: every sound on the board.
    pub async fn soundboard(&self) -> Result<Vec<SoundInfo>, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/soundboard")));
        parse_json(self.send(req).await?).await
    }

    /// Add a sound to the board. `wav` must be 16-bit mono 24 kHz, at most 5 seconds and 256 KB.
    pub async fn upload_board_sound(&self, wav: Vec<u8>, name: &str, emoji: Option<&str>) -> Result<SoundInfo, ClientError> {
        let mut form = Form::new().text("name", name.to_string());
        if let Some(emoji) = emoji.map(str::trim).filter(|emoji| !emoji.is_empty()) {
            form = form.text("emoji", emoji.to_string());
        }
        let part = Part::bytes(wav).file_name("sound.wav".to_string()).mime_str("audio/wav")?;
        let req = self.authorized(self.http.post(self.url("/api/soundboard"))).multipart(form.part("file", part));
        parse_json(self.send(req).await?).await
    }

    /// One sound's WAV bytes; cached by id on the server, so repeat calls can send `etag`.
    pub async fn download_board_sound_cached(&self, sound_id: Uuid, etag: Option<&str>) -> Result<(Option<Bytes>, Option<String>), ClientError> {
        let mut req = self.authorized(self.http.get(self.url(&format!("/api/soundboard/{sound_id}/file"))));
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let resp = self.send(req).await?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok((None, etag.map(str::to_string)));
        }
        let resp = check_status(resp).await?;
        let tag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
        Ok((Some(resp.bytes().await?), tag))
    }

    /// Remove a sound: its uploader or a moderator may.
    pub async fn delete_board_sound(&self, sound_id: Uuid) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url(&format!("/api/soundboard/{sound_id}"))));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Room music library: the tracks the server keeps plus temporary uploads still queued.
    pub async fn music_library(&self) -> Result<Vec<MusicTrack>, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/music/tracks")));
        parse_json(self.send(req).await?).await
    }

    /// Upload a track. Metadata goes before the file so the server knows the quota class up front.
    pub async fn upload_music_track(
        &self,
        bytes: Vec<u8>,
        filename: &str,
        title: &str,
        artist: Option<&str>,
        duration_ms: u64,
        keep: bool,
    ) -> Result<MusicTrack, ClientError> {
        let mut form = Form::new()
            .text("title", title.to_string())
            .text("duration_ms", duration_ms.to_string())
            .text("keep", if keep { "true" } else { "false" });
        if let Some(artist) = artist.map(str::trim).filter(|artist| !artist.is_empty()) {
            form = form.text("artist", artist.to_string());
        }
        let part = Part::bytes(bytes).file_name(filename.to_string());
        form = form.part("file", part);
        let req = self
            .authorized(self.http.post(self.url("/api/music/tracks")))
            .timeout(MUSIC_TRANSFER_TIMEOUT)
            .multipart(form);
        parse_json(self.send(req).await?).await
    }

    pub async fn delete_music_track(&self, track_id: Uuid) -> Result<(), ClientError> {
        let req = self.authorized(self.http.delete(self.url(&format!("/api/music/tracks/{track_id}"))));
        check_status(self.send(req).await?).await?;
        Ok(())
    }

    /// Streams a track into `dest` through `dest.part` and renames it when complete. Returns the bytes written.
    pub async fn download_music_file(&self, track_id: Uuid, dest: &Path) -> Result<u64, ClientError> {
        let req = self
            .authorized(self.http.get(self.url(&format!("/api/music/tracks/{track_id}/file"))))
            .timeout(MUSIC_TRANSFER_TIMEOUT);
        let mut resp = check_status(self.send(req).await?).await?;
        let mut part_name = dest.as_os_str().to_owned();
        part_name.push(".part");
        let part = PathBuf::from(part_name);
        let written = match write_stream(&mut resp, &part).await {
            Ok(written) => written,
            Err(error) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(error);
            }
        };
        tokio::fs::rename(&part, dest)
            .await
            .map_err(|e| ClientError::Network(format!("Could not save music file: {e}")))?;
        Ok(written)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn authorized(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => req.header("Authorization", format!("Bearer {token}")),
            None => req,
        }
    }
}

async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response, ClientError> {
    let status = resp.status();
    if status.is_success() {
        Ok(resp)
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(ClientError::Http {
            status: status.as_u16(),
            body: error_text(&body),
        })
    }
}

async fn parse_json<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, ClientError> {
    let resp = check_status(resp).await?;
    Ok(resp.json().await?)
}

/// Copies the body chunk by chunk into `part`. The file is closed before this returns.
async fn write_stream(resp: &mut reqwest::Response, part: &Path) -> Result<u64, ClientError> {
    let io_error = |e: std::io::Error| ClientError::Network(format!("Could not save music file: {e}"));
    let mut file = tokio::fs::File::create(part).await.map_err(io_error)?;
    let mut written = 0u64;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await.map_err(io_error)?;
        written = written.saturating_add(chunk.len() as u64);
    }
    file.flush().await.map_err(io_error)?;
    Ok(written)
}

/// QUIC address of a server: the host of its REST URL and the port the server announced,
/// falling back to [`resolve_quic_addr`]'s guess for servers that do not announce one.
pub fn resolve_quic_addr_with_port(
    server_url: &str,
    quic_port: Option<u16>,
) -> Result<(SocketAddr, String), ClientError> {
    let Some(port) = quic_port else {
        return resolve_quic_addr(server_url);
    };
    let url = reqwest::Url::parse(server_url).map_err(|e| ClientError::Network(e.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ClientError::Network("Missing host in server_url".to_string()))?;
    // The client endpoint and default server bind are IPv4. DNS often returns
    // ::1 first for localhost; prefer a compatible address when both exist.
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| ClientError::Network(e.to_string()))?
        .min_by_key(|addr| !addr.is_ipv4())
        .ok_or_else(|| ClientError::Network(format!("Could not resolve host: {host}")))?;
    Ok((addr, host.to_string()))
}

/// Derive the QUIC address of a server from its REST URL.
///
/// By default the FinchSpeak server runs REST on port 8443 (or 8080) and QUIC on port 4433.
pub fn resolve_quic_addr(server_url: &str) -> Result<(SocketAddr, String), ClientError> {
    let url = reqwest::Url::parse(server_url).map_err(|e| ClientError::Network(e.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ClientError::Network("Missing host in server_url".to_string()))?;
    let port = url.port().unwrap_or(8443);
    let quic_port = if port == 8443 || port == 8080 { 4433 } else { port };

    let addr_str = format!("{}:{}", host, quic_port);
    let addr = addr_str
        .to_socket_addrs()
        .map_err(|e| ClientError::Network(e.to_string()))?
        .min_by_key(|addr| !addr.is_ipv4())
        .ok_or_else(|| ClientError::Network(format!("Could not resolve host: {host}")))?;

    Ok((addr, host.to_string()))
}


/// The server answers errors as `{"error": "...", "status": N}`; show people the sentence.
fn error_text(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| json.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod error_text_tests {
    use super::error_text;

    #[test]
    fn the_servers_json_error_becomes_its_sentence() {
        assert_eq!(error_text(r#"{"error":"A valid invite code is required to register","status":403}"#), "A valid invite code is required to register");
    }

    #[test]
    fn anything_else_is_shown_as_it_is() {
        assert_eq!(error_text("Bad Gateway"), "Bad Gateway");
        assert_eq!(error_text(r#"{"message":"x"}"#), r#"{"message":"x"}"#);
        assert_eq!(error_text(""), "");
    }
}
