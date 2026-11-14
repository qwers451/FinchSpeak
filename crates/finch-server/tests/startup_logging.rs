//! Startup warnings must reach the persistent log, even though logging uses configuration values.

use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ServerProcess(Child);

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn persistent_log(dir: &Path) -> String {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn invalid_limits_warn_once_in_the_real_servers_persistent_log() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    fs::write(dir.path().join("finch-server.toml"), r#"
[server]
host = "127.0.0.1"
http_port = 0
quic_port = 0
[database]
url = "sqlite::memory:"
[auth]
jwt_secret = "startup-logging-test-secret-0123456789"
[backup]
enabled = false
[logging]
dir = "logs"
[limits]
media_burst_bytes = 0
auth_hash_concurrency = 0
"#)?;
    let stdout_path = dir.path().join("stdout.txt");
    let stderr_path = dir.path().join("stderr.txt");
    let mut server = ServerProcess(Command::new(env!("CARGO_BIN_EXE_finch-server"))
        .current_dir(dir.path())
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout_path)?)
        .stderr(fs::File::create(&stderr_path)?)
        .spawn()?);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let stdout = fs::read_to_string(&stdout_path)?;
        if stdout.contains(" ready. HTTPS: ") {
            break;
        }
        if let Some(status) = server.0.try_wait()? {
            panic!("server exited before ready ({status}): {stdout}\n{}", fs::read_to_string(&stderr_path)?);
        }
        assert!(Instant::now() < deadline, "server never became ready: {stdout}\n{}", fs::read_to_string(&stderr_path)?);
        std::thread::sleep(Duration::from_millis(50));
    }
    // Seeing the post-launch log entry proves the non-blocking writer has flushed the earlier
    // startup warnings too. It also exercises the REST/QUIC setup that used to revalidate limits.
    let logs = dir.path().join("logs");
    let log = loop {
        let log = persistent_log(&logs);
        if log.contains("REST API listening on") {
            break log;
        }
        assert!(Instant::now() < deadline, "persistent startup log was not written: {log}");
        std::thread::sleep(Duration::from_millis(50));
    };
    for warning in [
        "[limits] media_burst_bytes 0 is below 2048; using 2048",
        "[limits] auth_hash_concurrency is 0; using 1",
    ] {
        assert_eq!(log.matches(warning).count(), 1, "startup must persist each clamp warning once: {log}");
    }
    Ok(())
}
