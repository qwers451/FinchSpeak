//! REST API client for authentication, channels, message history and files.

use std::net::{SocketAddr, ToSocketAddrs};

use bytes::Bytes;
use finch_protocol::models::{AttachmentInfo, HistoryMessage};
use reqwest::multipart::{Form, Part};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ClientError;

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

/// HTTP client bound to one FinchSpeak server.
#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
    token: Option<String>,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Result<Self, ClientError> {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            token: None,
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
        let req = self
            .http
            .post(self.url("/api/auth/register"))
            .json(&serde_json::json!({
                "username": username,
                "display_name": display_name,
                "password": password
            }));
        parse_json(req.send().await?).await
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<AuthResponse, ClientError> {
        let req = self
            .http
            .post(self.url("/api/auth/login"))
            .json(&serde_json::json!({
                "username": username,
                "password": password
            }));
        parse_json(req.send().await?).await
    }

    pub async fn me(&self) -> Result<UserDto, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/auth/me")));
        parse_json(req.send().await?).await
    }

    pub async fn list_channels(&self) -> Result<Vec<ChannelInfoDto>, ClientError> {
        let req = self.authorized(self.http.get(self.url("/api/channels")));
        parse_json(req.send().await?).await
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
        parse_json(req.send().await?).await
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
        parse_json(req.send().await?).await
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
        parse_json(req.send().await?).await
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
        parse_json(req.send().await?).await
    }

    pub async fn download_file(&self, id: Uuid) -> Result<Bytes, ClientError> {
        let req = self.authorized(self.http.get(self.url(&format!("/api/files/{id}"))));
        let resp = check_status(req.send().await?).await?;
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
            body,
        })
    }
}

async fn parse_json<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, ClientError> {
    let resp = check_status(resp).await?;
    Ok(resp.json().await?)
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
