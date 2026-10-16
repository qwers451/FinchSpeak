//! UDP impairment proxy: packet loss and random per-packet delay (which also reorders).

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::net::UdpSocket;
use tokio::task::{JoinHandle, JoinSet};

const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(10);
const MAX_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Impairment applied independently to every packet in both directions.
#[derive(Debug, Clone, Copy)]
pub struct NetemConfig {
    /// Probability of dropping a packet (0.0..=1.0).
    pub loss: f64,
    /// Uniform random delay range in milliseconds `(min, max)`.
    pub delay_ms: (u64, u64),
    /// RNG seed for reproducible runs.
    pub seed: u64,
    /// Packets shorter than this pass untouched and do not use up random numbers. QUIC
    /// acknowledgements are small and their number varies from run to run; leaving them out
    /// makes the loss pattern of the (larger) media packets the same every run.
    pub min_packet_size: usize,
}

/// Proxy between one client and `upstream`. Stops when dropped.
pub struct UdpImpairmentProxy {
    pub listen_addr: SocketAddr,
    tasks: Vec<JoinHandle<()>>,
    enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl UdpImpairmentProxy {
    pub async fn start(upstream: SocketAddr, cfg: NetemConfig) -> anyhow::Result<Self> {
        Self::start_with(upstream, cfg, true).await
    }

    /// Like `start`, but the impairment only begins when `set_enabled(true)` is called. Until then
    /// every packet passes untouched and no random number is used, so the loss pattern that follows
    /// starts from the same point whatever the connection set-up looked like.
    pub async fn start_paused(upstream: SocketAddr, cfg: NetemConfig) -> anyhow::Result<Self> {
        Self::start_with(upstream, cfg, false).await
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, std::sync::atomic::Ordering::SeqCst);
    }

    async fn start_with(upstream: SocketAddr, cfg: NetemConfig, enabled: bool) -> anyhow::Result<Self> {
        let enabled = Arc::new(std::sync::atomic::AtomicBool::new(enabled));
        let listen = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
        let up = Arc::new(UdpSocket::bind("127.0.0.1:0").await?);
        up.connect(upstream).await?;
        let listen_addr = listen.local_addr()?;
        let client_addr: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(None));
        // One random sequence per direction: how the two directions interleave in time changes from
        // run to run, and sharing one sequence would let that decide which packets are dropped.
        let rng = Arc::new(Mutex::new(StdRng::seed_from_u64(cfg.seed)));
        let rng_back = Arc::new(Mutex::new(StdRng::seed_from_u64(cfg.seed ^ 0x9E37_79B9_7F4A_7C15)));

        // Client → upstream
        let to_server = {
            let (listen, up, client_addr, rng, enabled) = (listen.clone(), up.clone(), client_addr.clone(), rng.clone(), enabled.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65_536];
                // Own delayed sends so dropping the proxy also cancels them.
                let mut delayed = JoinSet::new();
                let mut retry_delay = INITIAL_RETRY_DELAY;
                loop {
                    let received = tokio::select! {
                        received = listen.recv_from(&mut buf) => received,
                        _ = delayed.join_next(), if !delayed.is_empty() => continue,
                    };
                    let (n, from) = match received {
                        Ok(packet) => packet,
                        Err(error) => {
                            if retry_receive(error, "client", &mut retry_delay).await {
                                continue;
                            }
                            break;
                        }
                    };
                    retry_delay = INITIAL_RETRY_DELAY;
                    if let Ok(mut addr) = client_addr.lock() {
                        *addr = Some(from);
                    }
                    if let Some(delay) = impair(&rng, &cfg, n, &enabled) {
                        let up = up.clone();
                        let data = buf[..n].to_vec();
                        delayed.spawn(async move {
                            tokio::time::sleep(delay).await;
                            let _ = up.send(&data).await;
                        });
                    }
                }
            })
        };

        // Upstream → client
        let to_client = {
            let (listen, up, client_addr, rng, enabled) = (listen, up, client_addr, rng_back, enabled.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65_536];
                let mut delayed = JoinSet::new();
                let mut retry_delay = INITIAL_RETRY_DELAY;
                loop {
                    let received = tokio::select! {
                        received = up.recv(&mut buf) => received,
                        _ = delayed.join_next(), if !delayed.is_empty() => continue,
                    };
                    let n = match received {
                        Ok(n) => n,
                        Err(error) => {
                            if retry_receive(error, "upstream", &mut retry_delay).await {
                                continue;
                            }
                            break;
                        }
                    };
                    retry_delay = INITIAL_RETRY_DELAY;
                    let Some(dest) = client_addr.lock().ok().and_then(|a| *a) else {
                        continue;
                    };
                    if let Some(delay) = impair(&rng, &cfg, n, &enabled) {
                        let listen = listen.clone();
                        let data = buf[..n].to_vec();
                        delayed.spawn(async move {
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
            enabled,
        })
    }
}

/// UDP can report a previous datagram's ICMP failure on the next receive.
/// Those errors do not invalidate the socket; persistent local socket errors do.
async fn retry_receive(error: io::Error, direction: &str, delay: &mut Duration) -> bool {
    if !matches!(
        error.kind(),
        io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::TimedOut
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable
    ) {
        tracing::error!(%error, direction, "UDP proxy receive failed; stopping direction");
        return false;
    }
    tracing::warn!(%error, direction, ?delay, "UDP proxy receive failed; retrying");
    tokio::time::sleep(*delay).await;
    *delay = (*delay * 2).min(MAX_RETRY_DELAY);
    true
}

/// `None` = drop the packet, otherwise the delay before forwarding it.
fn impair(rng: &Mutex<StdRng>, cfg: &NetemConfig, packet_len: usize, enabled: &std::sync::atomic::AtomicBool) -> Option<Duration> {
    if packet_len < cfg.min_packet_size || !enabled.load(std::sync::atomic::Ordering::SeqCst) {
        return Some(Duration::ZERO);
    }
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
