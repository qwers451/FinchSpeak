//! # FinchSpeak Server
//!
//! High-performance QUIC SFU and Axum REST API server for FinchSpeak.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use finch_server::config::ServerConfig;
use finch_server::launch::launch;

const CONFIG_FILE: &str = "finch-server.toml";

/// Log to the console and to daily files in `config.logging.dir` (old files are deleted).
/// The returned guard flushes the file on exit.
fn init_logging(config: &ServerConfig) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,finch_server=debug"));
    let console = tracing_subscriber::fmt::layer().with_filter(filter());

    let appender = (|| -> Result<_> {
        // Rotation prunes old logs before opening a writer. Its directory must
        // already exist, otherwise a successful first start prints a false error.
        std::fs::create_dir_all(&config.logging.dir)?;
        Ok(tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("finch-server")
            .filename_suffix("log")
            .max_log_files(config.logging.keep_files.max(1))
            .build(&config.logging.dir)?)
    })();
    match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer).with_filter(filter());
            tracing_subscriber::registry().with(console).with(file).init();
            Some(guard)
        }
        Err(e) => {
            tracing_subscriber::registry().with(console).init();
            tracing::warn!("Log files are disabled ({e}); logging to the console only");
            None
        }
    }
}

/// Picks the directory to run from when the server is started from elsewhere.
/// `None` keeps the current directory; `Some(exe_dir)` means the config lives
/// next to the executable and the server should switch there.
fn working_dir_for_config(cwd: &Path, exe_dir: &Path, name: &str) -> Option<PathBuf> {
    if cwd.join(name).exists() {
        None
    } else if exe_dir.join(name).exists() {
        Some(exe_dir.to_path_buf())
    } else {
        None
    }
}

/// Switches the working directory to the executable's folder when the config is only there.
/// Must run before the config is loaded, so relative `./data/...` paths resolve there too.
fn switch_to_config_dir() -> Result<Option<PathBuf>> {
    let cwd = std::env::current_dir().context("Failed to read the current directory")?;
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return Ok(None);
    };
    match working_dir_for_config(&cwd, &exe_dir, CONFIG_FILE) {
        Some(dir) => {
            std::env::set_current_dir(&dir)
                .with_context(|| format!("Failed to switch to the configuration folder {dir:?}"))?;
            Ok(Some(dir))
        }
        None => Ok(None),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 0. Run from the folder that holds the configuration
    let switched_to = switch_to_config_dir()?;

    // 1. Load configuration
    let config_path = Path::new(CONFIG_FILE);
    let config = ServerConfig::load_or_default(Some(config_path))
        .context("Failed to load server configuration")?;
    let _log_guard = init_logging(&config);
    if let Some(dir) = switched_to {
        tracing::info!("Using the configuration next to the executable: {dir:?}");
    }

    // 2. Initialize database, REST API and QUIC server
    let mut server = match launch(config).await {
        Ok(server) => server,
        Err(error) => {
            // Returning anyhow's error only writes stderr. Keep the full chain
            // in the persistent log as well, so a closed console loses no cause.
            tracing::error!(error = ?error, "Server startup failed");
            return Err(error);
        }
    };
    println!(
        "FinchSpeak {} ready. HTTPS: {} | QUIC: {}\nCertificate SHA-256: {}\nKeep this window open. Press Ctrl+C to stop.",
        env!("CARGO_PKG_VERSION"), server.http_addr, server.quic_addr, server.certificate_fingerprint
    );

    // 3. Wait for shutdown signal (Ctrl+C) or an unexpected server exit
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown signal received, terminating server...");
        }
        _ = server.wait_for_exit() => {}
    }

    server.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod working_dir {
    use std::fs;

    use super::working_dir_for_config;

    #[test]
    fn config_next_to_exe_wins_when_cwd_has_none() {
        let name = "finch-server.toml";
        let cwd = tempfile::tempdir().unwrap();
        let exe_dir = tempfile::tempdir().unwrap();

        // Config only next to the executable: run from the executable's folder.
        fs::write(exe_dir.path().join(name), "").unwrap();
        assert_eq!(
            working_dir_for_config(cwd.path(), exe_dir.path(), name),
            Some(exe_dir.path().to_path_buf())
        );

        // Config in the current directory always wins.
        fs::write(cwd.path().join(name), "").unwrap();
        assert_eq!(working_dir_for_config(cwd.path(), exe_dir.path(), name), None);

        // No config anywhere: keep the current directory.
        let empty_cwd = tempfile::tempdir().unwrap();
        let empty_exe = tempfile::tempdir().unwrap();
        assert_eq!(working_dir_for_config(empty_cwd.path(), empty_exe.path(), name), None);
    }
}
