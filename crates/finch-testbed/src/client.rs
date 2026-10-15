//! Headless FinchSpeak client built on `finch-client-core`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use dashmap::DashMap;
use finch_client_core::api::{ApiClient, UserDto};
use finch_client_core::error::ClientError;
use finch_client_core::session::{connect, ConnectParams, Session, SessionEvent};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_protocol::models::ChannelType;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::server::TestServer;

/// Password used for every bench account.
pub const TEST_PASSWORD: &str = "password_123";

/// Number of recent events quoted in a timeout error.
const RECENT_EVENTS: usize = 10;

pub struct TestClient {
    pub api: ApiClient,
    pub session: Session,
    pub user: UserDto,
    events: mpsc::Receiver<SessionEvent>,
    datagrams: VecDeque<Bytes>,
    recent: VecDeque<String>,
}

/// Open a QUIC session with an arbitrary token (fresh TOFU store).
pub async fn connect_with_token(
    server: &TestServer,
    token: &str,
) -> Result<(Session, mpsc::Receiver<SessionEvent>), ClientError> {
    connect(
        ConnectParams {
            quic_addr: server.quic_addr,
            hostname: "localhost".to_string(),
            token: token.to_string(),
        },
        Arc::new(DashMap::new()),
    )
    .await
}

impl TestClient {
    /// Register `username` (display name = username) and open a QUIC session.
    pub async fn register(server: &TestServer, username: &str) -> anyhow::Result<Self> {
        let mut api = ApiClient::new(&server.http_url)?;
        let auth = api.register(username, TEST_PASSWORD, username).await?;
        api.set_token(auth.token.clone());
        let (session, events) = connect_with_token(server, &auth.token).await?;
        Ok(Self {
            api,
            session,
            user: auth.user,
            events,
            datagrams: VecDeque::new(),
            recent: VecDeque::new(),
        })
    }

    /// Open a new QUIC session with the stored token, replacing the current one.
    pub async fn reconnect(&mut self, server: &TestServer) -> anyhow::Result<()> {
        let token = self
            .api
            .token()
            .ok_or_else(|| anyhow::anyhow!("client has no token"))?
            .to_string();
        let (session, events) = connect_with_token(server, &token).await?;
        self.session = session;
        self.events = events;
        self.datagrams.clear();
        Ok(())
    }

    pub async fn send(&self, msg: ClientMessage) -> anyhow::Result<()> {
        self.session
            .control_tx
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("control channel closed"))
    }

    /// Wait for a control message matching `pred`; other control messages are skipped,
    /// datagrams are kept for [`Self::drain_datagrams`].
    pub async fn expect_control<T>(
        &mut self,
        timeout: Duration,
        mut pred: impl FnMut(&ServerMessage) -> Option<T>,
    ) -> anyhow::Result<T> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let event = match tokio::time::timeout_at(deadline, self.events.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => anyhow::bail!("session event stream closed; recent: {:?}", self.recent),
                Err(_) => anyhow::bail!(
                    "timeout after {timeout:?} waiting for control message; recent: {:?}",
                    self.recent
                ),
            };
            if let SessionEvent::Control(msg) = &event
                && let Some(found) = pred(msg)
            {
                return Ok(found);
            }
            self.store(event);
        }
    }

    /// Fail if a control message matching `pred` arrives within `within`.
    pub async fn expect_no_control(
        &mut self,
        within: Duration,
        mut pred: impl FnMut(&ServerMessage) -> bool,
    ) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + within;
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, self.events.recv()).await {
            if let SessionEvent::Control(msg) = &event
                && pred(msg)
            {
                anyhow::bail!("unexpected control message: {msg:?}");
            }
            self.store(event);
        }
        Ok(())
    }

    /// Wait up to `timeout` for the next event of any kind (used for uni-stream checks).
    pub async fn next_event(&mut self, timeout: Duration) -> anyhow::Result<SessionEvent> {
        if let Some(d) = self.datagrams.pop_front() {
            return Ok(SessionEvent::Datagram(d));
        }
        match tokio::time::timeout(timeout, self.events.recv()).await {
            Ok(Some(event)) => Ok(event),
            Ok(None) => anyhow::bail!("session event stream closed"),
            Err(_) => anyhow::bail!("timeout after {timeout:?} waiting for an event"),
        }
    }

    /// All datagrams received so far (non-blocking).
    pub fn drain_datagrams(&mut self) -> Vec<Bytes> {
        while let Ok(event) = self.events.try_recv() {
            self.store(event);
        }
        self.datagrams.drain(..).collect()
    }

    /// First channel of `kind` from the server state received at authentication.
    pub fn channel(&self, kind: ChannelType) -> (Uuid, u32) {
        self.session
            .initial_state
            .channels
            .iter()
            .find(|c| c.channel_type == kind)
            .map(|c| (c.id, c.short_id))
            .unwrap_or_else(|| panic!("server has no {kind:?} channel"))
    }

    fn store(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Datagram(d) => self.datagrams.push_back(d),
            other => {
                if self.recent.len() == RECENT_EVENTS {
                    self.recent.pop_front();
                }
                self.recent.push_back(format!("{other:?}"));
            }
        }
    }
}
