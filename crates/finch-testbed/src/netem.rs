//! UDP impairment proxy: packet loss and random per-packet delay (which also reorders).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

/// Impairment applied independently to every packet in both directions.
#[derive(Debug, Clone, Copy)]
pub struct NetemConfig {
    /// Probability of dropping a packet (0.0..=1.0).
    pub loss: f64,
    /// Uniform random delay range in milliseconds `(min, max)`.
    pub delay_ms: (u64, u64),
    /// RNG seed for reproducible runs.
    pub seed: u64,
}

/// Proxy between one client and `upstream`. Stops when dropped.
pub struct UdpImpairmentProxy {
    pub listen_addr: SocketAddr,
    tasks: Vec<JoinHandle<()>>,
}

impl UdpImpairmentProxy {
    pub async fn start(upstream: SocketAddr, cfg: NetemConfig) -> anyhow::Result<Self> {
        let listen = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
        let up = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
        up.connect(upstream).await?;
        let listen_addr = listen.local_addr()?;
        let client_addr: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));
        let rng = Arc::new(Mutex::new(StdRng::seed_from_u64(cfg.seed)));

        // Client → upstream
        let to_server = {
            let (listen, up, client_addr, rng) = (listen.clone(), up.clone(), client_addr.clone(), rng.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65_536];
                while let Ok((n, from)) = listen.recv_from(&mut buf).await {
                    if let Ok(mut addr) = client_addr.lock() {
                        *addr = Some(from);
                    }
                    if let Some(delay) = impair(&rng, &cfg) {
                        let up = up.clone();
                        let data = buf[..n].to_vec();
                        tokio::spawn(async move {
                            tokio::time::sleep(delay).await;
                            let _ = up.send(&data).await;
                        });
                    }
                }
            })
        };

        // Upstream → client
        let to_client = {
            let (listen, up, client_addr, rng) = (listen, up, client_addr, rng);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65_536];
                while let Ok(n) = up.recv(&mut buf).await {
                    let Some(dest) = client_addr.lock().ok().and_then(|a| *a) else {
                        continue;
                    };
                    if let Some(delay) = impair(&rng, &cfg) {
                        let listen = listen.clone();
                        let data = buf[..n].to_vec();
                        tokio::spawn(async move {
                            tokio::time::sleep(delay).await;
                            let _ = listen.send_to(&data, dest).await;
                        });
                    }
                }
            })
        };

        Ok(Self {
            listen_addr,
            tasks: vec![to_server, to_client],
        })
    }
}

/// `None` = drop the packet, otherwise the delay before forwarding it.
fn impair(rng: &Mutex<StdRng>, cfg: &NetemConfig) -> Option<Duration> {
    let mut rng = rng.lock().ok()?;
    if rng.gen_bool(cfg.loss.clamp(0.0, 1.0)) {
        return None;
    }
    let (min, max) = cfg.delay_ms;
    Some(Duration::from_millis(rng.gen_range(min..=max.max(min))))
}

impl Drop for UdpImpairmentProxy {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
