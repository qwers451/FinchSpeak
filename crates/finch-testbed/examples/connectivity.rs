//! Read-only QUIC connectivity probe: no login, registration, room or microphone access.
//! cargo run -p finch-testbed --example connectivity -- hostname ip:4433 colon-SHA256
use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};
use dashmap::DashMap;
use finch_client_core::tofu::create_client_config_with;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 4, "usage: connectivity hostname ip:4433 colon-SHA256");
    let pins = Arc::new(DashMap::new());
    pins.insert(args[1].clone(), args[3].clone());
    let config = create_client_config_with(&args[1], pins, true, Arc::new(Mutex::new(None)))?;
    let address: std::net::SocketAddr = args[2].parse()?;
    let bind = if address.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let mut endpoint = quinn::Endpoint::client(bind.parse()?)?;
    endpoint.set_default_client_config(config);
    for attempt in 1..=3 {
        let started = Instant::now();
        let connection = tokio::time::timeout(Duration::from_secs(8),
            endpoint.connect(address, &args[1])?).await??;
        println!("QUIC attempt={attempt} handshake_ms={:.1} rtt_ms={:.1} max_datagram={} pinned_certificate=PASS",
            started.elapsed().as_secs_f64() * 1000.0,
            connection.rtt().as_secs_f64() * 1000.0,
            connection.max_datagram_size().unwrap_or(0));
        connection.close(0u32.into(), b"read-only connectivity probe complete");
    }
    tokio::time::timeout(Duration::from_secs(3), endpoint.wait_idle()).await?;
    Ok(())
}
