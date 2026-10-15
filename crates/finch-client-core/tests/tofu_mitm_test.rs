//! A server that presents a certificate it does not hold the key for must be rejected (AUD-002).

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use finch_client_core::tofu::create_client_config;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

/// Presents certificate A but signs the handshake with an unrelated key B — what an attacker
/// replaying the server's public certificate would do.
#[derive(Debug)]
struct ForeignKeyResolver(Arc<CertifiedKey>);

impl ResolvesServerCert for ForeignKeyResolver {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}

#[tokio::test]
async fn certificate_without_matching_key_is_rejected() -> anyhow::Result<()> {
    let key_a = rcgen::KeyPair::generate()?;
    let cert_a = CertificateDer::from(
        rcgen::CertificateParams::new(vec!["localhost".to_string()])?.self_signed(&key_a)?,
    );
    let key_b = rcgen::KeyPair::generate()?;
    let key_b_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_b.serialize_der()));
    let signer = rustls::crypto::ring::sign::any_supported_type(&key_b_der)?;
    let resolver = ForeignKeyResolver(Arc::new(CertifiedKey::new(vec![cert_a], signer)));

    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));
    tls.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(tls)?,
    ));
    let server = quinn::Endpoint::server(server_config, "127.0.0.1:0".parse()?)?;
    let server_addr = server.local_addr()?;
    tokio::spawn(async move {
        if let Some(incoming) = server.accept().await {
            let _ = incoming.await;
        }
    });

    let mut client = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    client.set_default_client_config(create_client_config("localhost", Arc::new(DashMap::new()))?);
    let result = tokio::time::timeout(Duration::from_secs(5), client.connect(server_addr, "localhost")?).await?;
    assert!(result.is_err(), "handshake signed with a foreign key was accepted");
    Ok(())
}
