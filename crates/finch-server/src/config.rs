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
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            quic_port: default_quic_port(),
            http_port: default_http_port(),
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

/// Authentication and JWT settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "default_jwt_expiration_hours", alias = "token_expiry_hours")]
    pub jwt_expiration_hours: i64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: default_jwt_secret(),
            jwt_expiration_hours: default_jwt_expiration_hours(),
        }
    }
}

fn default_jwt_secret() -> String {
    "finchspeak_jwt_secret_change_in_production_32bytes_long!".to_string()
}

fn default_jwt_expiration_hours() -> i64 {
    72 // 3 days
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
}
