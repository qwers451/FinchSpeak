//! Connections are time-boxed and capped until they sign in: a silent connection is closed at the
//! sign-in deadline, and a client address can hold only a few unsigned connections at once.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use dashmap::DashMap;
use finch_client_core::tofu::create_client_config_with;
use finch_protocol::codec::{decode_message, encode_message, CodecError};
use finch_protocol::messages::{ClientMessage, ServerMessage};
use finch_server::quic::{CLOSE_PREAUTH_REFUSED, CLOSE_PREAUTH_TIMEOUT};
use finch_testbed::client::{connect_to, TEST_PASSWORD};
use finch_testbed::server::TestServer;
use quinn::{ApplicationClose, Connection, ConnectionError, Endpoint, VarInt};

/// Completes the QUIC handshake without sending anything else. The endpoint must outlive the
/// connection, so both are returned.
async fn dial(
    quic_addr: SocketAddr,
) -> anyhow::Result<(Endpoint, Result<Connection, ConnectionError>)> {
    let config = create_client_config_with(
        "localhost",
        Arc::new(DashMap::new()),
        false,
        Arc::new(Mutex::new(None)),
    )?;
    let mut endpoint = Endpoint::client(SocketAddr::from(([0, 0, 0, 0], 0)))?;
    endpoint.set_default_client_config(config);
    let outcome = endpoint.connect(quic_addr, "localhost")?.await;
    Ok((endpoint, outcome))
}

/// Waits (at most `within`) for the server to close the connection and returns its close code.
async fn closed_with(
    outcome: Result<Connection, ConnectionError>,
    within: Duration,
) -> anyhow::Result<VarInt> {
    let error = match outcome {
        Ok(conn) => tokio::time::timeout(within, conn.closed())
            .await
            .map_err(|_| anyhow::anyhow!("connection stayed open for {within:?}"))?,
        Err(error) => error,
    };
    match error {
        ConnectionError::ApplicationClosed(ApplicationClose { error_code, .. }) => Ok(error_code),
        other => anyhow::bail!("expected an application close, got: {other}"),
    }
}

/// Sends Authenticate on a new control stream and waits for the server to accept it.
async fn sign_in(conn: &Connection, token: &str) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.open_bi().await?;
    let encoded = encode_message(&ClientMessage::Authenticate {
        token: token.to_string(),
        supports_voice_extensions: true,
        client_level: finch_protocol::CLIENT_LEVEL_1_7,
    })?;
    send.write_all(&encoded).await?;
    let mut frames = BytesMut::new();
    let mut chunk = vec![0u8; 4096];
    loop {
        let n = recv
            .read(&mut chunk)
            .await?
            .ok_or_else(|| anyhow::anyhow!("stream closed before the sign-in verdict"))?;
        frames.extend_from_slice(&chunk[..n]);
        match decode_message::<ServerMessage>(&mut frames) {
            Ok(ServerMessage::Authenticated { .. }) => return Ok(()),
            Ok(_) => anyhow::bail!("server answered Authenticate with something other than Authenticated"),
            Err(CodecError::Incomplete { .. }) => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_that_never_signs_in_is_closed_at_the_deadline() -> anyhow::Result<()> {
    let server = TestServer::start().await?;

    let started = Instant::now();
    let (_endpoint, outcome) = dial(server.quic_addr).await?;
    let code = closed_with(outcome, Duration::from_secs(20)).await?;
    let elapsed = started.elapsed();

    assert_eq!(code, VarInt::from(CLOSE_PREAUTH_TIMEOUT), "closed with the sign-in timeout code");
    assert!(
        elapsed >= Duration::from_secs(9) && elapsed <= Duration::from_secs(13),
        "closed after {elapsed:?}, expected about 10 s"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ninth_unsigned_connection_from_one_address_waits_for_a_signed_in_slot() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let api = server.api()?;
    let auth = api.register("gatekeeper", TEST_PASSWORD, "gatekeeper").await?;

    // Eight connections from 127.0.0.1 take the whole per-address allowance and never sign in.
    let mut held = Vec::new();
    for i in 0..8 {
        let (endpoint, outcome) = dial(server.quic_addr).await?;
        let conn = outcome.map_err(|e| anyhow::anyhow!("unsigned connection {i} was refused: {e}"))?;
        held.push((endpoint, conn));
    }

    // The ninth is closed right away, before any sign-in.
    let (_endpoint, outcome) = dial(server.quic_addr).await?;
    let code = closed_with(outcome, Duration::from_secs(2)).await?;
    assert_eq!(code, VarInt::from(CLOSE_PREAUTH_REFUSED), "the ninth is refused with the limit code");

    // One of the eight signs in, which frees its slot: a new client from the same address is accepted.
    sign_in(&held[0].1, &auth.token).await?;
    let fresh = connect_to(server.quic_addr, &auth.token).await;
    assert!(fresh.is_ok(), "after a sign-in frees a slot, a new connection is accepted: {:?}", fresh.err());
    Ok(())
}
