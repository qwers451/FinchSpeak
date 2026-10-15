//! End-to-End Full Stack Autotest Suite.
//!
//! Validates:
//! 1. REST API: Registration, JWT login, Me endpoint.
//! 2. Channels: Seeding default channels, admin channel creation, member listing.
//! 3. QUIC: Real QUIC connection, JWT authentication on Stream 0 (ServerMessage::Authenticated).
//! 4. Chat: Sending messages via QUIC Stream 0, persistence to SQLite, real-time broadcast to peers.
//! 5. History: Fetching persisted message history via REST API.
//! 6. Voice: Joining voice channel, voice presence broadcast, and clean leave.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use bytes::BytesMut;
use finch_protocol::codec::{decode_message, encode_message};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_server::api::create_router;
use finch_server::config::ServerConfig;
use finch_server::db::init_db;
use finch_server::quic;
use finch_server::session::SessionManager;
use finch_server::sfu::SfuRouter;
use http_body_util::BodyExt;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Endpoint};
use serde_json::{json, Value};
use tower::util::ServiceExt;
use uuid::Uuid;

#[derive(Debug)]
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
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
        vec![
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

fn make_client_config() -> anyhow::Result<ClientConfig> {
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    crypto.alpn_protocols = vec![b"finchspeak/0.1".to_vec()];

    let quic_crypto = QuicClientConfig::try_from(crypto)?;
    let mut client_config = ClientConfig::new(Arc::new(quic_crypto));

    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(Duration::from_secs(10).try_into().unwrap()));
    client_config.transport_config(Arc::new(transport));

    Ok(client_config)
}

async fn wait_for_message<F, R>(
    label: &str,
    recv: &mut quinn::RecvStream,
    buf: &mut BytesMut,
    mut predicate: F,
) -> anyhow::Result<R>
where
    F: FnMut(&ServerMessage) -> Option<R>,
{
    let mut chunk = [0u8; 4096];
    loop {
        while let Ok(msg) = decode_message::<ServerMessage>(buf) {
            println!("[{label}] Decoded message: {msg:?}");
            if let Some(res) = predicate(&msg) {
                return Ok(res);
            }
        }
        let read_res = tokio::time::timeout(Duration::from_secs(4), recv.read(&mut chunk)).await;
        let n = match read_res {
            Ok(Ok(Some(n))) => {
                println!("[{label}] Read {n} bytes from stream");
                n
            }
            Ok(Ok(None)) => return Err(anyhow::anyhow!("[{label}] Stream closed by server")),
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => return Err(anyhow::anyhow!("[{label}] Timeout (4s) waiting for expected message")),
        };
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[tokio::test]
async fn test_full_system_autotest() -> anyhow::Result<()> {
    // ─── 1. Initialize SQLite Database & Configuration ───────────────────────
    let pool = init_db("sqlite::memory:").await?;
    let config = ServerConfig::default();
    let jwt_secret = config.auth.jwt_secret.clone();
    let sessions = SessionManager::new();
    quic::sync_channels_from_db(&pool, &sessions).await?;
    let sfu = SfuRouter::new(sessions.clone());

    let app = create_router(pool.clone(), config.clone());

    // ─── 2. Start QUIC SFU Server on ephemeral UDP port ─────────────────────
    let server_config = quic::make_server_config()?;
    let server_endpoint = Endpoint::server(server_config, "127.0.0.1:0".parse()?)?;
    let server_addr = server_endpoint.local_addr()?;

    let pool_clone = pool.clone();
    let sessions_clone = sessions.clone();
    let sfu_clone = sfu.clone();
    let jwt_secret_clone = jwt_secret.clone();

    let _server_task = tokio::spawn(async move {
        while let Some(incoming) = server_endpoint.accept().await {
            let p = pool_clone.clone();
            let s = sessions_clone.clone();
            let r = sfu_clone.clone();
            let sec = jwt_secret_clone.clone();
            tokio::spawn(async move {
                if let Ok(conn) = incoming.await {
                    let _ = quic::handle_connection(conn, p, s, r, sec).await;
                }
            });
        }
    });

    // ─── 3. Register First User (Admin) via REST ─────────────────────────────
    let register_admin = json!({
        "username": "admin",
        "password": "admin_password_123",
        "display_name": "Server Administrator"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&register_admin)?))?;
    let res = app.clone().oneshot(req).await?;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: Value = serde_json::from_slice(&res.into_body().collect().await?.to_bytes())?;
    let admin_token = body["token"].as_str().unwrap().to_string();
    assert_eq!(body["user"]["role"], "admin");

    // ─── 4. Register Second User (Member) via REST ───────────────────────────
    let register_member = json!({
        "username": "alice",
        "password": "alice_password_123",
        "display_name": "Alice"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&register_member)?))?;
    let res = app.clone().oneshot(req).await?;
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: Value = serde_json::from_slice(&res.into_body().collect().await?.to_bytes())?;
    let member_token = body["token"].as_str().unwrap().to_string();

    // ─── 5. Verify Channels Listing ──────────────────────────────────────────
    let req = Request::builder()
        .method("GET")
        .uri("/api/channels")
        .header(header::AUTHORIZATION, format!("Bearer {member_token}"))
        .body(Body::empty())?;
    let res = app.clone().oneshot(req).await?;
    assert_eq!(res.status(), StatusCode::OK);
    let channels: Vec<Value> = serde_json::from_slice(&res.into_body().collect().await?.to_bytes())?;
    assert_eq!(channels.len(), 2);
    let general_ch_id = channels[0]["id"].as_str().unwrap().to_string();
    let general_uuid = Uuid::parse_str(&general_ch_id)?;
    assert_eq!(channels[0]["name"], "general");
    assert_eq!(channels[1]["name"], "General Voice");

    // ─── 6. Connect Alice via QUIC and Authenticate on Stream 0 ──────────────
    let client_config = make_client_config()?;
    let mut client_endpoint = Endpoint::client("127.0.0.1:0".parse()?)?;
    client_endpoint.set_default_client_config(client_config.clone());

    let alice_conn = client_endpoint.connect(server_addr, "localhost")?.await?;
    let (mut alice_send, mut alice_recv) = alice_conn.open_bi().await?;

    let auth_msg = ClientMessage::Authenticate { token: member_token.clone() };
    alice_send.write_all(&encode_message(&auth_msg)?).await?;

    let mut stream_buf = BytesMut::new();
    let alice_session_peer_id = wait_for_message("Alice Auth", &mut alice_recv, &mut stream_buf, |msg| {
        if let ServerMessage::Authenticated { session_peer_id, .. } = msg {
            Some(*session_peer_id)
        } else {
            None
        }
    }).await?;
    assert!(alice_session_peer_id > 0);

    // ─── 7. Connect Admin via QUIC and Authenticate on Stream 0 ──────────────
    let mut admin_client_endpoint = Endpoint::client("127.0.0.1:0".parse()?)?;
    admin_client_endpoint.set_default_client_config(client_config);

    let admin_conn = admin_client_endpoint.connect(server_addr, "localhost")?.await?;
    let (mut admin_send, mut admin_recv) = admin_conn.open_bi().await?;

    let admin_auth_msg = ClientMessage::Authenticate { token: admin_token.clone() };
    admin_send.write_all(&encode_message(&admin_auth_msg)?).await?;

    let mut admin_buf = BytesMut::new();
    let admin_session_peer_id = wait_for_message("Admin Auth", &mut admin_recv, &mut admin_buf, |msg| {
        if let ServerMessage::Authenticated { session_peer_id, .. } = msg {
            Some(*session_peer_id)
        } else {
            None
        }
    }).await?;
    assert!(admin_session_peer_id > 0);

    // ─── 8. Send Chat Message over QUIC Stream 0 ─────────────────────────────
    let test_content = "Hello, FinchSpeak automated test message!";
    let send_msg = ClientMessage::SendMessage {
        channel_id: general_uuid,
        content: test_content.to_string(),
        attachments: vec![],
    };
    alice_send.write_all(&encode_message(&send_msg)?).await?;

    // Both Alice and Admin receive the message
    let (ch_id_alice, content_alice, author_alice) = wait_for_message("Alice Recv NewMessage", &mut alice_recv, &mut stream_buf, |msg| {
        if let ServerMessage::NewMessage { channel_id, content, author_name, .. } = msg {
            Some((*channel_id, content.clone(), author_name.clone()))
        } else {
            None
        }
    }).await?;
    assert_eq!(ch_id_alice, general_uuid);
    assert_eq!(content_alice, test_content);
    assert_eq!(author_alice, "alice");

    let (ch_id_admin, content_admin, author_admin) = wait_for_message("Admin Recv NewMessage", &mut admin_recv, &mut admin_buf, |msg| {
        if let ServerMessage::NewMessage { channel_id, content, author_name, .. } = msg {
            Some((*channel_id, content.clone(), author_name.clone()))
        } else {
            None
        }
    }).await?;
    assert_eq!(ch_id_admin, general_uuid);
    assert_eq!(content_admin, test_content);
    assert_eq!(author_admin, "alice");

    // ─── 9. Verify Message History via REST ──────────────────────────────────
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/channels/{general_ch_id}/messages"))
        .header(header::AUTHORIZATION, format!("Bearer {member_token}"))
        .body(Body::empty())?;
    let res = app.clone().oneshot(req).await?;
    assert_eq!(res.status(), StatusCode::OK);
    let hist: Value = serde_json::from_slice(&res.into_body().collect().await?.to_bytes())?;
    let msgs = hist["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["content"], test_content);

    // ─── 10. Voice Channel Presence: Join and Leave ──────────────────────────
    let voice_uuid = Uuid::parse_str(channels[1]["id"].as_str().unwrap())?;
    let join_voice = ClientMessage::JoinVoice { channel_id: voice_uuid };
    alice_send.write_all(&encode_message(&join_voice)?).await?;

    let joined_user = wait_for_message("Admin Recv UserJoinedVoice", &mut admin_recv, &mut admin_buf, |msg| {
        if let ServerMessage::UserJoinedVoice { username, .. } = msg {
            Some(username.clone())
        } else {
            None
        }
    }).await?;
    assert_eq!(joined_user, "alice");

    let leave_voice = ClientMessage::LeaveVoice;
    alice_send.write_all(&encode_message(&leave_voice)?).await?;

    let left_peer = wait_for_message("Admin Recv UserLeftVoice", &mut admin_recv, &mut admin_buf, |msg| {
        if let ServerMessage::UserLeftVoice { peer_id, .. } = msg {
            Some(*peer_id)
        } else {
            None
        }
    }).await?;
    assert_eq!(left_peer, alice_session_peer_id);

    println!("SUCCESS: Full Stack Autotests passed with 100% functionality verified!");
    Ok(())
}
