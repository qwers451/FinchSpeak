//! In-process server launch: ephemeral ports, REST over TLS only, clean shutdown.

use finch_server::config::ServerConfig;
use finch_server::launch::launch;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn ephemeral_config() -> ServerConfig {
    let dir = std::env::temp_dir().join(format!("finch-launch-{}", uuid::Uuid::new_v4()));
    let mut config = ServerConfig::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.http_port = 0;
    config.server.quic_port = 0;
    config.database.url = "sqlite::memory:".to_string();
    config.storage.upload_dir = dir.join("uploads").to_string_lossy().to_string();
    config.auth.jwt_secret = "launch-test-secret-0123456789abcdef".to_string();
    config.server.tls_dir = dir.join("tls").to_string_lossy().to_string();
    config
}

#[tokio::test]
async fn launch_binds_ephemeral_ports_and_shuts_down() -> anyhow::Result<()> {
    let server = launch(ephemeral_config()).await?;
    let http_addr = server.http_addr;
    assert_ne!(http_addr.port(), 0);
    assert_ne!(server.quic_addr.port(), 0);

    // The REST listener speaks TLS; the certificate is self-signed, so this test does not verify it
    // (certificate pinning is covered by finch-client-core's rest_tls_test).
    let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()?;
    let status = client
        .get(format!("https://{http_addr}/api/channels"))
        .send()
        .await?
        .status();
    assert_eq!(status.as_u16(), 401);

    server.shutdown().await;
    TcpListener::bind(http_addr).await?;
    Ok(())
}

/// Credentials must never be accepted in the clear: a plaintext HTTP request to the REST port
/// gets no HTTP response at all.
#[tokio::test]
async fn plaintext_http_is_not_served() -> anyhow::Result<()> {
    let server = launch(ephemeral_config()).await?;

    let mut stream = TcpStream::connect(server.http_addr).await?;
    stream
        .write_all(b"GET /api/channels HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    let mut response = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        stream.read_to_end(&mut response),
    )
    .await;
    assert!(
        !response.starts_with(b"HTTP/"),
        "plaintext request got an HTTP answer: {:?}",
        String::from_utf8_lossy(&response)
    );

    server.shutdown().await;
    Ok(())
}
