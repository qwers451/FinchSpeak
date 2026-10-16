//! In-process FinchSpeak server on ephemeral ports with an on-disk temporary database.

use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use finch_client_core::api::ApiClient;
use finch_server::config::ServerConfig;
use finch_server::launch::{launch, RunningServer};
use tempfile::TempDir;

/// Upload limit used by the bench, small enough to test rejection quickly.
pub const TEST_MAX_FILE_SIZE_MB: usize = 1;

pub struct TestServer {
    /// `https://127.0.0.1:<port>` — the REST listener is TLS-only.
    pub http_url: String,
    pub quic_addr: SocketAddr,
    /// SHA-256 fingerprint clients pin for this server (stable across restarts).
    pub certificate_fingerprint: String,
    pins: Arc<DashMap<String, String>>,
    config: ServerConfig,
    running: Option<RunningServer>,
    _dir: TempDir,
}

impl TestServer {
    /// Start a server on 127.0.0.1 with free ports and a fresh database.
    pub async fn start() -> anyhow::Result<Self> {
        Self::start_with(|_| {}).await
    }

    /// Like [`Self::start`], with a hook to adjust the configuration before launch.
    pub async fn start_with(adjust: impl FnOnce(&mut ServerConfig)) -> anyhow::Result<Self> {
        crate::init_tracing();
        let dir = tempfile::tempdir()?;
        let db_path = dir.path().join("finch.db");
        let upload_dir = dir.path().join("uploads");

        let mut config = ServerConfig::default();
        config.server.host = "127.0.0.1".to_string();
        config.server.http_port = 0;
        config.server.quic_port = 0;
        config.database.url = format!(
            "sqlite://{}?mode=rwc",
            db_path.to_string_lossy().replace('\\', "/")
        );
        config.storage.upload_dir = upload_dir.to_string_lossy().to_string();
        config.storage.max_file_size_mb = TEST_MAX_FILE_SIZE_MB;
        config.server.tls_dir = dir.path().join("tls").to_string_lossy().to_string();
        config.auth.jwt_secret = format!("testbed-secret-{}", uuid::Uuid::new_v4());
        config.backup.enabled = false;
        // Most scenarios create many users; invitation-only registration has its own tests.
        config.auth.registration = finch_server::config::RegistrationMode::Open;
        adjust(&mut config);

        let running = launch(config.clone()).await?;
        config.server.http_port = running.http_addr.port();
        config.server.quic_port = running.quic_addr.port();

        let certificate_fingerprint = running.certificate_fingerprint.clone();
        let pins = Arc::new(DashMap::new());
        pins.insert("127.0.0.1".to_string(), certificate_fingerprint.clone());

        Ok(Self {
            http_url: format!("https://{}", running.http_addr),
            quic_addr: running.quic_addr,
            certificate_fingerprint,
            pins,
            config,
            running: Some(running),
            _dir: dir,
        })
    }

    /// REST client that already trusts this server's certificate (as after the user confirmed it).
    pub fn api(&self) -> anyhow::Result<ApiClient> {
        Ok(ApiClient::with_tofu(&self.http_url, self.pins.clone())?)
    }

    /// REST client with an empty pin store: the first request reports `UntrustedServer`.
    pub fn api_without_pin(&self) -> anyhow::Result<ApiClient> {
        Ok(ApiClient::with_tofu(&self.http_url, Arc::new(DashMap::new()))?)
    }

    /// JWT secret the server signs tokens with.
    pub fn jwt_secret(&self) -> &str {
        &self.config.auth.jwt_secret
    }

    /// Stop the server and start it again on the same ports and database.
    pub async fn restart(&mut self) -> anyhow::Result<()> {
        self.stop().await;
        self.running = Some(launch(self.config.clone()).await?);
        Ok(())
    }

    /// Stop the server (idempotent).
    pub async fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.shutdown().await;
        }
    }
}
