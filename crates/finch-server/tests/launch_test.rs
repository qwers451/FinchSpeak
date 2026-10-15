//! In-process server launch: ephemeral ports, REST reachable, clean shutdown.

use std::sync::Arc;
use std::time::Duration;

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

#[derive(Debug)]
struct AcceptAnyCert;

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// After a server task exits on its own (`wait_for_exit` returned), `shutdown` must not panic.
#[tokio::test]
async fn shutdown_after_unexpected_exit_does_not_panic() -> anyhow::Result<()> {
    let mut config = ServerConfig::default();
    config.server.host = "127.0.0.1".to_string();
    config.server.http_port = 0;
    config.server.quic_port = 0;
    config.database.url = "sqlite::memory:".to_string();
    config.storage.upload_dir = std::env::temp_dir()
        .join(format!("finch-launch-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let mut server = launch(config).await?;

    // A failed handshake (wrong ALPN) currently ends the QUIC accept loop (AUD-004).
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"wrong".to_vec()];
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls)?,
    )));
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        endpoint.connect(server.quic_addr, "localhost")?,
    )
    .await;

    tokio::time::timeout(Duration::from_secs(5), server.wait_for_exit()).await?;
    let joined = tokio::spawn(server.shutdown()).await;
    assert!(joined.is_ok(), "shutdown panicked: {joined:?}");
    Ok(())
}
