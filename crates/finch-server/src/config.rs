//! Server configuration loading and defaults.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Root server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerConfig {
    #[serde(default)]
    pub server: NetworkConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub backup: BackupConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub music: MusicConfig,
    #[serde(default)]
    pub soundboard: SoundboardConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
}

/// Media rate limits per participant (the SFU drops what exceeds them and counts it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LimitsConfig {
    /// Media bytes one participant may send per second. 1.5 MB/s = 12 Mbit/s: above the 10 Mbit/s
    /// screen cap plus voice.
    pub media_bytes_per_sec: u64,
    /// Bytes that may be sent at once after a quiet spell.
    pub media_burst_bytes: u64,
    /// Media datagrams one participant may send per second (the burst is one second's worth).
    pub media_packets_per_sec: u64,
    /// Keyframe bytes one participant may send per second. The burst is one maximum keyframe.
    pub media_keyframe_bytes_per_sec: u64,
    /// Seconds a new connection has to sign in (the Authenticate message) before it is closed.
    pub preauth_timeout_secs: u64,
    /// Connections per client address that have not signed in yet; more are closed at once.
    pub preauth_per_ip: usize,
    /// Connections in total that have not signed in yet; more are closed at once.
    pub preauth_total: usize,
    /// Argon2 hashes or checks running at once. Each allocates about 19 MiB, so this bounds memory;
    /// further sign-ins and password changes wait their turn.
    pub auth_hash_concurrency: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            media_bytes_per_sec: 1_500_000,
            media_burst_bytes: 262_144,
            media_packets_per_sec: 2000,
            media_keyframe_bytes_per_sec: 1_000_000,
            preauth_timeout_secs: 10,
            preauth_per_ip: 8,
            preauth_total: 256,
            auth_hash_concurrency: 2,
        }
    }
}

/// Smallest burst that still lets a full media datagram through with room to spare.
pub const MIN_MEDIA_BURST_BYTES: u64 = 2048;

impl LimitsConfig {
    /// Clamp values that would make media unusable (a burst smaller than a datagram, no packets at
    /// all), warning about each one that is changed.
    pub fn validated(mut self) -> Self {
        if self.media_burst_bytes < MIN_MEDIA_BURST_BYTES {
            tracing::warn!(
                "[limits] media_burst_bytes {} is below {MIN_MEDIA_BURST_BYTES}; using {MIN_MEDIA_BURST_BYTES}",
                self.media_burst_bytes
            );
            self.media_burst_bytes = MIN_MEDIA_BURST_BYTES;
        }
        if self.media_packets_per_sec < 1 {
            tracing::warn!("[limits] media_packets_per_sec is 0; using 1");
            self.media_packets_per_sec = 1;
        }
        if self.media_keyframe_bytes_per_sec < 1 {
            tracing::warn!("[limits] media_keyframe_bytes_per_sec is 0; using 1");
            self.media_keyframe_bytes_per_sec = 1;
        }
        if self.preauth_timeout_secs < 1 {
            tracing::warn!("[limits] preauth_timeout_secs is 0; using 1");
            self.preauth_timeout_secs = 1;
        }
        if self.preauth_per_ip < 1 {
            tracing::warn!("[limits] preauth_per_ip is 0; using 1");
            self.preauth_per_ip = 1;
        }
        if self.preauth_total < 1 {
            tracing::warn!("[limits] preauth_total is 0; using 1");
            self.preauth_total = 1;
        }
        if self.auth_hash_concurrency < 1 {
            tracing::warn!("[limits] auth_hash_concurrency is 0; using 1");
            self.auth_hash_concurrency = 1;
        }
        self
    }
}

/// The shared soundboard: sounds are stored in the database; `enabled = false` hides the feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundboardConfig {
    pub enabled: bool,
    pub max_sounds: usize,
}

impl Default for SoundboardConfig {
    fn default() -> Self {
        Self { enabled: true, max_sounds: 48 }
    }
}

/// Music files live under uploads/music unless a dedicated disk is configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicConfig {
    pub enabled: bool,
    pub storage_dir: Option<String>,
    pub max_track_mb: u64,
    pub library_quota_mb: u64,
    pub temp_quota_mb_per_user: u64,
    pub temp_quota_mb_total: u64,
    pub max_queue: usize,
    pub max_track_minutes: u64,
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            storage_dir: None,
            max_track_mb: 30,
            library_quota_mb: 2048,
            temp_quota_mb_per_user: 200,
            temp_quota_mb_total: 512,
            max_queue: 100,
            max_track_minutes: 180,
        }
    }
}

/// Periodic copies of the database (SQLite `VACUUM INTO`, consistent while the server runs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_backup_dir")]
    pub dir: String,
    #[serde(default = "default_backup_interval_hours")]
    pub interval_hours: u64,
    /// How many of the newest backups to keep.
    #[serde(default = "default_backup_keep")]
    pub keep: usize,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: default_backup_dir(),
            interval_hours: default_backup_interval_hours(),
            keep: default_backup_keep(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_backup_dir() -> String {
    "./data/backups".to_string()
}

fn default_backup_interval_hours() -> u64 {
    24
}

fn default_backup_keep() -> usize {
    7
}

/// Log files (rotated daily).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_dir")]
    pub dir: String,
    /// How many daily files to keep.
    #[serde(default = "default_log_keep")]
    pub keep_files: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self { dir: default_log_dir(), keep_files: default_log_keep() }
    }
}

fn default_log_dir() -> String {
    "./data/logs".to_string()
}

fn default_log_keep() -> usize {
    14
}

/// Network binding and ports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_quic_port")]
    pub quic_port: u16,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    /// Directory with the persistent TLS certificate and key (`cert.der`, `key.der`).
    #[serde(default = "default_tls_dir")]
    pub tls_dir: String,
    /// Extra browser origins allowed to call the HTTP API (CORS), e.g. `"https://finch.example.com"`.
    /// The desktop app's own origins are always allowed. Invalid entries are skipped with a warning.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            quic_port: default_quic_port(),
            http_port: default_http_port(),
            tls_dir: default_tls_dir(),
            allowed_origins: Vec::new(),
        }
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_quic_port() -> u16 {
    4433
}

fn default_http_port() -> u16 {
    8443
}

fn default_tls_dir() -> String {
    "./data/tls".to_string()
}

/// Database settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
        }
    }
}

fn default_database_url() -> String {
    "sqlite://finchspeak.db?mode=rwc".to_string()
}

/// File storage and attachment limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_upload_dir")]
    pub upload_dir: String,
    #[serde(default = "default_max_file_size_mb")]
    pub max_file_size_mb: usize,
    /// Total size of the files one user may keep, in MB.
    #[serde(default = "default_user_quota_mb")]
    pub user_quota_mb: usize,
    /// Uploads one user may start per minute.
    #[serde(default = "default_uploads_per_minute")]
    pub uploads_per_minute: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            upload_dir: default_upload_dir(),
            max_file_size_mb: default_max_file_size_mb(),
            user_quota_mb: default_user_quota_mb(),
            uploads_per_minute: default_uploads_per_minute(),
        }
    }
}

fn default_user_quota_mb() -> usize {
    1024
}

fn default_uploads_per_minute() -> usize {
    30
}

impl StorageConfig {
    /// Clamp values that would make uploads impossible (a quota or rate of 0), warning about each.
    pub fn validated(mut self) -> Self {
        if self.user_quota_mb < 1 {
            tracing::warn!("[storage] user_quota_mb is 0; using 1");
            self.user_quota_mb = 1;
        }
        if self.uploads_per_minute < 1 {
            tracing::warn!("[storage] uploads_per_minute is 0; using 1");
            self.uploads_per_minute = 1;
        }
        self
    }
}

fn default_upload_dir() -> String {
    "./data/uploads".to_string()
}

fn default_max_file_size_mb() -> usize {
    50
}

/// Who may create an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RegistrationMode {
    /// Anyone who can reach the server.
    Open,
    /// Only with a code from an admin (the very first account is always allowed: it becomes admin).
    #[default]
    Invite,
}

/// Authentication and JWT settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "default_jwt_expiration_hours", alias = "token_expiry_hours")]
    pub jwt_expiration_hours: i64,
    /// Where a generated secret is stored when `jwt_secret` is not set by the operator.
    #[serde(default = "default_jwt_secret_file")]
    pub jwt_secret_file: String,
    /// `invite` (default) or `open`.
    #[serde(default)]
    pub registration: RegistrationMode,
    /// Wrong passwords for one login (from one address) before it is locked.
    #[serde(default = "default_login_max_failures")]
    pub login_max_failures: u32,
    /// How long such a lock lasts.
    #[serde(default = "default_login_lockout_secs")]
    pub login_lockout_secs: u64,
    /// New accounts per address per hour.
    #[serde(default = "default_register_per_hour")]
    pub register_per_hour: usize,
    /// Messages one person may send per `message_window_secs`.
    #[serde(default = "default_message_limit")]
    pub message_limit: usize,
    #[serde(default = "default_message_window_secs")]
    pub message_window_secs: u64,
}

fn default_login_max_failures() -> u32 {
    5
}
fn default_login_lockout_secs() -> u64 {
    60
}
fn default_register_per_hour() -> usize {
    20
}
fn default_message_limit() -> usize {
    30
}
fn default_message_window_secs() -> u64 {
    10
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: default_jwt_secret(),
            jwt_expiration_hours: default_jwt_expiration_hours(),
            jwt_secret_file: default_jwt_secret_file(),
            registration: RegistrationMode::default(),
            login_max_failures: default_login_max_failures(),
            login_lockout_secs: default_login_lockout_secs(),
            register_per_hour: default_register_per_hour(),
            message_limit: default_message_limit(),
            message_window_secs: default_message_window_secs(),
        }
    }
}

fn default_jwt_secret() -> String {
    "finchspeak_jwt_secret_change_in_production_32bytes_long!".to_string()
}

fn default_jwt_expiration_hours() -> i64 {
    720 // 30 days, sliding: POST /api/auth/refresh extends it
}

fn default_jwt_secret_file() -> String {
    "./data/jwt_secret".to_string()
}

/// Prefix of the placeholder secret shipped in `deploy/finch-server.toml`.
const PLACEHOLDER_SECRET_PREFIX: &str = "CHANGE_ME";

impl AuthConfig {
    /// The secret used to sign JWTs.
    ///
    /// - A placeholder (`CHANGE_ME…`) is an operator mistake and stops startup.
    /// - An empty value or the well-known default from the repository is never used for
    ///   signing: a random 256-bit secret is generated once and stored in `jwt_secret_file`,
    ///   so tokens survive restarts.
    /// - Any other value is the operator's own secret and is used as is.
    pub fn resolve_jwt_secret(&self) -> Result<String> {
        let configured = self.jwt_secret.trim();
        if configured.starts_with(PLACEHOLDER_SECRET_PREFIX) {
            anyhow::bail!(
                "auth.jwt_secret is still the placeholder; set your own secret or remove the line to auto-generate one"
            );
        }
        if !configured.is_empty() && configured != default_jwt_secret() {
            return Ok(configured.to_string());
        }

        let path = Path::new(&self.jwt_secret_file);
        if let Err(e) = crate::secret_file::tighten_permissions(path) {
            tracing::warn!("Could not restrict permissions of {path:?}: {e}");
        }
        if let Ok(stored) = fs::read_to_string(path) {
            let stored = stored.trim();
            if !stored.is_empty() {
                return Ok(stored.to_string());
            }
        }

        let mut bytes = [0u8; 32];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
        let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("Failed to create directory for {:?}", path))?;
        }
        crate::secret_file::write_private(path, secret.as_bytes())
            .with_context(|| format!("Failed to write JWT secret to {:?}", path))?;
        tracing::warn!("No JWT secret configured; generated a new one and stored it in {:?}", path);
        Ok(secret)
    }
}

impl ServerConfig {
    /// Load configuration from a TOML file path or fallback to defaults.
    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(p) if p.exists() => {
                let content = fs::read_to_string(p)
                    .with_context(|| format!("Failed to read config file at {:?}", p))?;
                let config: Self = toml::from_str(&content)
                    .with_context(|| format!("Failed to parse TOML configuration from {:?}", p))?;
                Ok(config)
            }
            _ => Ok(Self::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_origins_default_to_empty_and_parse_from_the_server_section() {
        assert!(ServerConfig::default().server.allowed_origins.is_empty());
        let config: ServerConfig = toml::from_str("[server]\nallowed_origins = [\"https://finch.example.com\"]\n").unwrap();
        assert_eq!(config.server.allowed_origins, vec!["https://finch.example.com".to_string()]);
        let without: ServerConfig = toml::from_str("[server]\nquic_port = 4433\n").unwrap();
        assert!(without.server.allowed_origins.is_empty());
    }

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.server.quic_port, 4433);
        assert_eq!(config.server.http_port, 8443);
        assert_eq!(config.storage.max_file_size_mb, 50);
        assert_eq!(config.auth.jwt_expiration_hours, 720);
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
        [server]
        host = "127.0.0.1"
        quic_port = 5000
        http_port = 9000

        [storage]
        upload_dir = "/tmp/uploads"
        max_file_size_mb = 100

        [auth]
        token_expiry_hours = 48
        "#;

        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.quic_port, 5000);
        assert_eq!(config.server.http_port, 9000);
        assert_eq!(config.storage.upload_dir, "/tmp/uploads");
        assert_eq!(config.storage.max_file_size_mb, 100);
        assert_eq!(config.auth.jwt_expiration_hours, 48);
    }

    #[test]
    fn test_deploy_config_toml() {
        let toml_str = r#"
        [server]
        host = "0.0.0.0"
        quic_port = 4433
        http_port = 8443

        [database]
        url = "sqlite:///app/data/finchspeak.db?mode=rwc"

        [storage]
        upload_dir = "/app/data/uploads"
        max_file_size_mb = 50

        [auth]
        jwt_secret = "CHANGE_ME_SECURE_JWT_SECRET_32_BYTES_MINIMUM"
        token_expiry_hours = 72
        "#;

        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.quic_port, 4433);
        assert_eq!(config.server.http_port, 8443);
        assert_eq!(config.database.url, "sqlite:///app/data/finchspeak.db?mode=rwc");
        assert_eq!(config.storage.upload_dir, "/app/data/uploads");
        assert_eq!(config.storage.max_file_size_mb, 50);
        assert_eq!(config.auth.jwt_secret, "CHANGE_ME_SECURE_JWT_SECRET_32_BYTES_MINIMUM");
        assert_eq!(config.auth.jwt_expiration_hours, 72);
    }

    #[test]
    fn implausible_limits_are_clamped_to_usable_values() {
        let clamped = LimitsConfig {
            media_bytes_per_sec: 1000,
            media_burst_bytes: 0,
            media_packets_per_sec: 0,
            media_keyframe_bytes_per_sec: 0,
            preauth_timeout_secs: 0,
            preauth_per_ip: 0,
            preauth_total: 0,
            auth_hash_concurrency: 0,
        }
        .validated();
        assert_eq!(clamped.auth_hash_concurrency, 1, "a zero concurrency would wait forever");
        assert_eq!(clamped.media_burst_bytes, MIN_MEDIA_BURST_BYTES);
        assert_eq!(clamped.media_packets_per_sec, 1);
        assert_eq!(clamped.media_keyframe_bytes_per_sec, 1);
        assert_eq!(clamped.media_bytes_per_sec, 1000, "other values are kept as configured");
        assert_eq!((clamped.preauth_timeout_secs, clamped.preauth_per_ip, clamped.preauth_total), (1, 1, 1));
        assert_eq!(LimitsConfig::default().validated().media_burst_bytes, 262_144);
        assert_eq!(LimitsConfig::default().media_keyframe_bytes_per_sec, 1_000_000);
    }

    #[test]
    fn limits_section_is_optional_and_overrides_single_keys() {
        let defaults = ServerConfig::default();
        assert_eq!(defaults.limits.media_bytes_per_sec, 1_500_000);
        assert_eq!(defaults.limits.media_burst_bytes, 262_144);
        assert_eq!(defaults.limits.media_packets_per_sec, 2000);

        let without_section: ServerConfig = toml::from_str("[server]\nquic_port = 4433\n").unwrap();
        assert_eq!(without_section.limits.media_packets_per_sec, 2000, "existing configs keep working");

        let partial: ServerConfig = toml::from_str("[limits]\nmedia_bytes_per_sec = 900000\n").unwrap();
        assert_eq!(partial.limits.media_bytes_per_sec, 900_000);
        assert_eq!(partial.limits.media_burst_bytes, 262_144);

        assert_eq!(defaults.limits.preauth_timeout_secs, 10);
        assert_eq!(defaults.limits.preauth_per_ip, 8);
        assert_eq!(defaults.limits.preauth_total, 256);
        let preauth: ServerConfig = toml::from_str("[limits]\npreauth_per_ip = 4\n").unwrap();
        assert_eq!(preauth.limits.preauth_per_ip, 4);
        assert_eq!(preauth.limits.preauth_total, 256);
    }

    fn temp_secret_file() -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("finch-jwt-{}", uuid::Uuid::new_v4()))
            .join("jwt_secret")
    }

    #[test]
    fn test_default_secret_is_replaced_and_persisted() {
        let auth = AuthConfig {
            jwt_secret_file: temp_secret_file().to_string_lossy().to_string(),
            ..AuthConfig::default()
        };

        let first = auth.resolve_jwt_secret().unwrap();
        assert_ne!(first, default_jwt_secret());
        assert!(first.len() >= 64, "secret too short: {}", first.len());

        // Restart: the same generated secret is read back, so issued tokens stay valid.
        let second = auth.resolve_jwt_secret().unwrap();
        assert_eq!(first, second);
    }

    /// The generated JWT secret is owner-only, and a secret file left world-readable by an
    /// older version is restricted at the next start. Unix only: Windows has no mode bits.
    #[cfg(unix)]
    #[test]
    fn test_jwt_secret_file_is_owner_only_and_tightened_on_start() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jwt_secret");
        let auth = AuthConfig {
            jwt_secret_file: path.to_string_lossy().to_string(),
            ..AuthConfig::default()
        };
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;

        let first = auth.resolve_jwt_secret().unwrap();
        assert_eq!(mode(&path), 0o600, "generated secret must be owner-only");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let second = auth.resolve_jwt_secret().unwrap();
        assert_eq!(mode(&path), 0o600, "existing secret must be tightened at startup");
        assert_eq!(first, second, "tightening must not change the secret");
    }

    #[test]
    fn test_placeholder_secret_is_rejected() {
        let auth = AuthConfig {
            jwt_secret: "CHANGE_ME_SECURE_JWT_SECRET_32_BYTES_MINIMUM".to_string(),
            jwt_secret_file: temp_secret_file().to_string_lossy().to_string(),
            ..AuthConfig::default()
        };
        assert!(auth.resolve_jwt_secret().is_err());
    }

    #[test]
    fn test_custom_secret_is_kept() {
        let auth = AuthConfig {
            jwt_secret: "a-real-operator-chosen-secret-0123456789abcdef".to_string(),
            jwt_secret_file: temp_secret_file().to_string_lossy().to_string(),
            ..AuthConfig::default()
        };
        assert_eq!(auth.resolve_jwt_secret().unwrap(), auth.jwt_secret);
    }
}

#[cfg(test)]
mod storage_limit_tests {
    use super::*;

    #[test]
    fn storage_upload_limits_have_defaults_and_are_clamped() {
        let defaults = ServerConfig::default().storage;
        assert_eq!((defaults.user_quota_mb, defaults.uploads_per_minute), (1024, 30));
        let partial: ServerConfig = toml::from_str("[storage]\nuser_quota_mb = 10\n").unwrap();
        assert_eq!((partial.storage.user_quota_mb, partial.storage.uploads_per_minute), (10, 30));
        let clamped = StorageConfig { user_quota_mb: 0, uploads_per_minute: 0, ..StorageConfig::default() }.validated();
        assert_eq!((clamped.user_quota_mb, clamped.uploads_per_minute), (1, 1));
    }
}
