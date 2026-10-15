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
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            quic_port: default_quic_port(),
            http_port: default_http_port(),
            tls_dir: default_tls_dir(),
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
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            upload_dir: default_upload_dir(),
            max_file_size_mb: default_max_file_size_mb(),
        }
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
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: default_jwt_secret(),
            jwt_expiration_hours: default_jwt_expiration_hours(),
            jwt_secret_file: default_jwt_secret_file(),
            registration: RegistrationMode::default(),
        }
    }
}

fn default_jwt_secret() -> String {
    "finchspeak_jwt_secret_change_in_production_32bytes_long!".to_string()
}

fn default_jwt_expiration_hours() -> i64 {
    72 // 3 days
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
        fs::write(path, &secret).with_context(|| format!("Failed to write JWT secret to {:?}", path))?;
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
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.server.quic_port, 4433);
        assert_eq!(config.server.http_port, 8443);
        assert_eq!(config.storage.max_file_size_mb, 50);
        assert_eq!(config.auth.jwt_expiration_hours, 72);
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
