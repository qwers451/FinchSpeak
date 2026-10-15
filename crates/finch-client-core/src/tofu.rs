//! TOFU (Trust On First Use) certificate verification and QUIC client configuration.

use std::sync::Arc;
use std::time::Duration;

use quinn::{ClientConfig, TransportConfig};
use ring::digest::{digest, SHA256};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use tracing::{debug, error, info};

use crate::error::ClientError;

/// TOFU (Trust On First Use) Certificate Verifier.
#[derive(Debug)]
pub struct TofuVerifier {
    pub hostname: String,
    pub saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
    crypto_provider: Arc<rustls::crypto::CryptoProvider>,
}

impl TofuVerifier {
    pub fn new(hostname: String, saved_fingerprints: Arc<dashmap::DashMap<String, String>>) -> Self {
        Self {
            hostname,
            saved_fingerprints,
            crypto_provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }

    pub fn compute_fingerprint(cert: &[u8]) -> String {
        let hash = digest(&SHA256, cert);
        hash.as_ref()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(":")
    }
}

impl ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let cert_fp = Self::compute_fingerprint(end_entity.as_ref());

        if let Some(existing_fp) = self.saved_fingerprints.get(&self.hostname) {
            if existing_fp.value() != &cert_fp {
                error!(
                    "TOFU verification failed for {}: expected {}, received {}",
                    self.hostname, existing_fp.value(), cert_fp
                );
                return Err(RustlsError::InvalidCertificate(
                    rustls::CertificateError::ApplicationVerificationFailure,
                ));
            }
            debug!("TOFU certificate match for {}", self.hostname);
        } else {
            info!(
                "First connection to {}: trusting certificate fingerprint {}",
                self.hostname, cert_fp
            );
            self.saved_fingerprints.insert(self.hostname.clone(), cert_fp);
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.crypto_provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Create Quinn client configuration with TOFU verifier and RFC 9221 datagram settings.
pub fn create_client_config(
    hostname: &str,
    saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
) -> Result<ClientConfig, ClientError> {
    let verifier = Arc::new(TofuVerifier::new(hostname.to_string(), saved_fingerprints));

    let mut tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    tls_config.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];

    let quic_client_config = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)
        .map_err(|e| ClientError::Quic(e.to_string()))?;
    let mut client_config = ClientConfig::new(Arc::new(quic_client_config));

    let mut transport = TransportConfig::default();
    transport.max_idle_timeout(Some(
        Duration::from_secs(15)
            .try_into()
            .map_err(|e: quinn::VarIntBoundsExceeded| ClientError::Quic(e.to_string()))?,
    ));
    transport.keep_alive_interval(Some(Duration::from_secs(4)));
    transport.datagram_receive_buffer_size(Some(2 * 1024 * 1024));
    transport.datagram_send_buffer_size(2 * 1024 * 1024);
    client_config.transport_config(Arc::new(transport));

    Ok(client_config)
}
