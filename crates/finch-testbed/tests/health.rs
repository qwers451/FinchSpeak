//! Server health panel: who may read it, what it counts, and what it reports on a real server.

use std::sync::atomic::Ordering;
use std::time::Duration;

use finch_client_core::error::ClientError;
use finch_server::health::{HISTORY_LEN, TRAFFIC};
use finch_testbed::client::{audio_datagram, TestClient};
use finch_testbed::server::TestServer;

const WAIT: Duration = Duration::from_secs(3);

fn status(error: &ClientError) -> Option<u16> {
    match error {
        ClientError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

/// (bytes in, bytes out, datagrams dropped) of the process-wide media counters.
fn traffic() -> (u64, u64, u64) {
    (
        TRAFFIC.bytes_in.load(Ordering::Relaxed),
        TRAFFIC.bytes_out.load(Ordering::Relaxed),
        TRAFFIC.datagrams_dropped.load(Ordering::Relaxed),
    )
}

#[tokio::test]
async fn health_is_for_the_administrator_only() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    let moderator = TestClient::register(&server, "moddy").await?;
    let member = TestClient::register(&server, "member").await?;
    boss.api.set_user_role(moderator.session.user_id, "moderator").await?;

    let report = boss.api.server_health().await?;
    assert!(report.now.system_total_bytes > 0, "system memory is measured");

    for (name, client) in [("moderator", &moderator), ("member", &member)] {
        let error = client.api.server_health().await.expect_err(name);
        assert_eq!(status(&error), Some(403), "{name} must not see the health panel");
    }
    Ok(())
}

#[tokio::test]
async fn health_counts_forwarded_media_in_both_directions() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let _boss = TestClient::register(&server, "boss").await?;
    let mut alice = TestClient::register(&server, "alice").await?;
    let mut bob = TestClient::register(&server, "bob").await?;
    let (_, short) = alice.join_voice().await?;
    bob.join_voice().await?;
    let alice_peer = alice.session.session_peer_id;

    let (in_before, out_before, _) = traffic();
    let datagram = audio_datagram(short, alice_peer, 1, 60);
    let len = datagram.len() as u64;
    alice.session.send_datagram(datagram)?;
    bob.wait_datagram(WAIT).await?;

    // The counter is updated right after the send, so give it a moment to show up.
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let (in_after, out_after, _) = traffic();
        if in_after >= in_before + len && out_after >= out_before + len {
            break;
        }
        anyhow::ensure!(tokio::time::Instant::now() < deadline, "traffic counters did not grow by {len} bytes");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

#[tokio::test]
async fn health_report_has_history_and_folder_sizes() -> anyhow::Result<()> {
    let server = TestServer::start().await?;
    let boss = TestClient::register(&server, "boss").await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let report = boss.api.server_health().await?;
    assert!(!report.history.is_empty(), "the sampler records its first sample at once");
    assert!(report.history.len() <= HISTORY_LEN);
    assert!(report.db_bytes > 0, "the database has pages");
    assert!(report.now.rss_bytes > 0, "this process has resident memory");
    assert!(report.uptime_secs < 3600);
    Ok(())
}
