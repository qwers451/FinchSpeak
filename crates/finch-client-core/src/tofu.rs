//! TOFU (Trust On First Use) certificate verification and QUIC client configuration.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::{ClientConfig, TransportConfig};
use ring::digest::{digest, SHA256};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use tracing::{debug, error, info};

use crate::error::ClientError;

/// Why a server certificate was not trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TofuIssue {
    /// First contact and the user has not confirmed the fingerprint yet.
    Unknown { fingerprint: String },
    /// The server presents a different certificate than the pinned one.
    Mismatch { expected: String, actual: String },
}

/// Shared slot where the verifier reports why it rejected a certificate.
pub type TofuIssueSlot = Arc<Mutex<Option<TofuIssue>>>;

/// TOFU (Trust On First Use) Certificate Verifier.
#[derive(Debug)]
pub struct TofuVerifier {
    pub hostname: String,
    pub saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
    /// When true, an unknown server is rejected until the user confirms its fingerprint;
    /// when false, the first fingerprint seen is pinned automatically.
    require_known: bool,
    issue: TofuIssueSlot,
    crypto_provider: Arc<rustls::crypto::CryptoProvider>,
}

impl TofuVerifier {
    /// Verifier that pins the first fingerprint seen automatically.
    pub fn new(hostname: String, saved_fingerprints: Arc<dashmap::DashMap<String, String>>) -> Self {
        Self::with_policy(hostname, saved_fingerprints, false, Arc::new(Mutex::new(None)))
    }

    pub fn with_policy(
        hostname: String,
        saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
        require_known: bool,
        issue: TofuIssueSlot,
    ) -> Self {
        Self {
            hostname,
            saved_fingerprints,
            require_known,
            issue,
            crypto_provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }

    fn reject(&self, issue: TofuIssue) -> RustlsError {
        if let Ok(mut slot) = self.issue.lock() {
            *slot = Some(issue);
        }
        RustlsError::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)
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

        let pinned = self.saved_fingerprints.get(&self.hostname).map(|e| e.value().clone());
        if let Some(existing_fp) = pinned {
            if existing_fp != cert_fp {
                error!(
                    "TOFU verification failed for {}: expected {}, received {}",
                    self.hostname, existing_fp, cert_fp
                );
                return Err(self.reject(TofuIssue::Mismatch {
                    expected: existing_fp,
                    actual: cert_fp,
                }));
            }
            debug!("TOFU certificate match for {}", self.hostname);
        } else if self.require_known {
            info!(
                "First connection to {}: fingerprint {} needs confirmation",
                self.hostname, cert_fp
            );
            return Err(self.reject(TofuIssue::Unknown { fingerprint: cert_fp }));
        } else {
            info!(
                "First connection to {}: trusting certificate fingerprint {}",
                self.hostname, cert_fp
            );
            self.saved_fingerprints.insert(self.hostname.clone(), cert_fp);
        }

        Ok(ServerCertVerified::assertion())
    }

    /// The pinned fingerprint only identifies the certificate; the handshake signature proves
    /// the server holds its private key. Both checks are required.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.crypto_provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.crypto_provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.crypto_provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Create Quinn client configuration that pins the first fingerprint seen automatically.
pub fn create_client_config(
    hostname: &str,
    saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
) -> Result<ClientConfig, ClientError> {
    create_client_config_with(hostname, saved_fingerprints, false, Arc::new(Mutex::new(None)))
}

/// Create Quinn client configuration with TOFU verifier and RFC 9221 datagram settings.
pub fn create_client_config_with(
    hostname: &str,
    saved_fingerprints: Arc<dashmap::DashMap<String, String>>,
    require_known: bool,
    issue: TofuIssueSlot,
) -> Result<ClientConfig, ClientError> {
    let verifier = Arc::new(TofuVerifier::with_policy(
        hostname.to_string(),
        saved_fingerprints,
        require_known,
        issue,
    ));

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


/// Load pinned fingerprints (`<hostname> <fingerprint>` per line). A missing file is empty.
pub fn load_fingerprints(path: &Path) -> std::io::Result<dashmap::DashMap<String, String>> {
    let store = dashmap::DashMap::new();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(store),
        Err(e) => return Err(e),
    };
    for line in content.lines() {
        if let Some((host, fp)) = line.trim().split_once(' ') {
            store.insert(host.to_string(), fp.trim().to_string());
        }
    }
    Ok(store)
}

/// Save pinned fingerprints, one `<hostname> <fingerprint>` per line, sorted by hostname.
pub fn save_fingerprints(path: &Path, store: &dashmap::DashMap<String, String>) -> std::io::Result<()> {
    let mut lines: Vec<String> = store
        .iter()
        .map(|e| format!("{} {}", e.key(), e.value()))
        .collect();
    lines.sort();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, lines.join("\n") + "\n")
}
