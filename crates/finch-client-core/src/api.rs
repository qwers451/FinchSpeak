//! REST API client for authentication, channels, message history and files.

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use finch_protocol::models::{AttachmentInfo, HistoryMessage, ServerState};
use reqwest::multipart::{Form, Part};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ClientError;
use crate::tofu::{TofuIssue, TofuIssueSlot, TofuVerifier};

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
    pub online: bool,
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

/// HTTP client bound to one FinchSpeak server.
#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
    token: Option<String>,
    trust_issue: TofuIssueSlot,
    hostname: String,
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
                "invite_code": invite_code
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

    pub async fn unban_user(&self, user_id: Uuid) -> Result<(), ClientError> {
        self.admin_action(user_id, "unban").await
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
                "password": password
            }));
        parse_json(self.send(req).await?).await
    }

    pub async fn me(&self) -> Result<UserDto, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/auth/me")));
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
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| ClientError::Network(e.to_string()))?
        .next()
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
        .next()
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
