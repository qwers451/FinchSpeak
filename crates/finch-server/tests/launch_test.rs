//! In-process server launch: ephemeral ports, REST reachable, clean shutdown.

use finch_server::config::ServerConfig;
use finch_server::launch::launch;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn http_status(addr: std::net::SocketAddr, path: &str) -> anyhow::Result<String> {
    let mut stream = TcpStream::connect(addr).await?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let text = String::from_utf8_lossy(&response);
    Ok(text.lines().next().unwrap_or_default().to_string())
}

#[tokio::test]
async fn launch_binds_ephemeral_ports_and_shuts_down() -> anyhow::Result<()> {
    let upload_dir = std::env::temp_dir().join(format!("finch-launch-{}", uuid::Uuid::new_v4()));

    let mut config = ServerConfig::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.http_port = 0;
    config.server.quic_port = 0;
    config.database.url = "sqlite::memory:".to_string();
    config.storage.upload_dir = upload_dir.to_string_lossy().to_string();

    let server = launch(config).await?;
    let http_addr = server.http_addr;
    assert_ne!(http_addr.port(), 0);
    assert_ne!(server.quic_addr.port(), 0);

    let status = http_status(http_addr, "/api/channels").await?;
    assert!(status.contains("401"), "unexpected status line: {status}");

    server.shutdown().await;
    TcpListener::bind(http_addr).await?;

    let _ = std::fs::remove_dir_all(upload_dir);
    Ok(())
}
