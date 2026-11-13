//! Server health: a sample every 5 seconds (CPU and memory of this process, system memory,
//! the container's memory limit, free disk under the data folder, sessions, voice users,
//! traffic and lost datagrams) with one hour of history kept in memory.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;
use sqlx::SqlitePool;
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::task::JoinHandle;

use crate::config::ServerConfig;
use crate::session::SessionManager;

/// Time between two samples.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
/// Samples kept: one hour at [`SAMPLE_INTERVAL`].
pub const HISTORY_LEN: usize = 720;
/// Memory above this share of the limit raises the `memory-high` warning.
const MEMORY_WARN_PERCENT: u64 = 85;
/// Less free disk than this raises the `disk-low` warning.
pub const DISK_LOW_BYTES: u64 = 1024 * 1024 * 1024;
/// cgroup v1 writes a number near 2^63 when there is no limit.
const UNLIMITED_FROM: u64 = 1 << 62;

/// Traffic and loss counters of the media router. Process-wide: every server in this process adds to them.
pub static TRAFFIC: Traffic = Traffic {
    bytes_in: AtomicU64::new(0),
    bytes_out: AtomicU64::new(0),
    datagrams_dropped: AtomicU64::new(0),
};

pub struct Traffic {
    /// Bytes of media received from clients.
    pub bytes_in: AtomicU64,
    /// Bytes of media handed to receivers.
    pub bytes_out: AtomicU64,
    /// Media that could not be handed to a receiver (too large, unsupported, connection gone).
    pub datagrams_dropped: AtomicU64,
}

/// One measurement.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HealthSample {
    /// Unix seconds.
    pub at: i64,
    pub cpu_percent: f32,
    pub rss_bytes: u64,
    pub system_used_bytes: u64,
    pub system_total_bytes: u64,
    pub container_limit_bytes: Option<u64>,
    pub disk_free_bytes: u64,
    pub sessions: u32,
    pub voice_users: u32,
    pub bytes_in_per_sec: u64,
    pub bytes_out_per_sec: u64,
    pub datagrams_dropped: u64,
}

/// What the admin panel shows.
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub now: HealthSample,
    /// The last hour, oldest first.
    pub history: Vec<HealthSample>,
    pub db_bytes: u64,
    /// Attachments, without the music library.
    pub uploads_bytes: u64,
    pub music_bytes: u64,
    pub uptime_secs: u64,
    /// Codes such as `memory-high` and `disk-low`; the client words them.
    pub warnings: Vec<String>,
}

/// Memory limit of the cgroup this process runs in; `max` (no limit) or a missing file gives `None`.
pub fn parse_cgroup_limit(text: &str) -> Option<u64> {
    let value = text.trim().parse::<u64>().ok()?;
    (value < UNLIMITED_FROM).then_some(value)
}

/// The container's memory limit from cgroup v2, then cgroup v1. Outside a container: `None`.
pub fn container_limit() -> Option<u64> {
    const FILES: [&str; 2] = ["/sys/fs/cgroup/memory.max", "/sys/fs/cgroup/memory/memory.limit_in_bytes"];
    FILES
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| parse_cgroup_limit(&text))
}

/// Free bytes on the disk that holds `dir`: the mount point that is the longest prefix of the path.
pub fn free_space_for<'a>(mounts: impl IntoIterator<Item = (&'a Path, u64)>, dir: &Path) -> u64 {
    mounts
        .into_iter()
        .filter(|(mount, _)| dir.starts_with(mount))
        .max_by_key(|(mount, _)| mount.as_os_str().len())
        .map_or(0, |(_, free)| free)
}

/// Warning codes for one sample.
pub fn warnings_for(sample: &HealthSample) -> Vec<String> {
    let mut warnings = Vec::new();
    let memory_limit = sample.container_limit_bytes.unwrap_or(sample.system_total_bytes);
    if memory_limit > 0 && sample.rss_bytes.saturating_mul(100) > memory_limit.saturating_mul(MEMORY_WARN_PERCENT) {
        warnings.push("memory-high".to_string());
    }
    if sample.disk_free_bytes < DISK_LOW_BYTES {
        warnings.push("disk-low".to_string());
    }
    warnings
}

/// Append a sample, dropping the oldest once the history is full.
pub fn push_sample(history: &mut VecDeque<HealthSample>, sample: HealthSample) {
    while history.len() >= HISTORY_LEN {
        history.pop_front();
    }
    history.push_back(sample);
}

/// Size of a folder tree in bytes (unreadable entries count as 0).
pub fn dir_size(path: &Path) -> u64 {
    crate::api::admin::dir_size(path)
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn absolute(path: &str) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path))
}

/// The state the sampler reuses between samples: CPU usage needs two readings of the same process.
struct Meter {
    system: System,
    disks: Disks,
    pid: Pid,
    cpus: f32,
    last_at: Instant,
    last_in: u64,
    last_out: u64,
}

/// Samples the server every [`SAMPLE_INTERVAL`] and answers the admin panel.
pub struct HealthMonitor {
    pool: SqlitePool,
    sessions: SessionManager,
    data_dir: PathBuf,
    upload_dir: PathBuf,
    music_dir: PathBuf,
    started: Instant,
    meter: Mutex<Meter>,
    history: Mutex<VecDeque<HealthSample>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for HealthMonitor {
    fn drop(&mut self) {
        if let Some(task) = locked(&self.task).take() {
            task.abort();
        }
    }
}

impl HealthMonitor {
    /// Start sampling. The sampler holds only a weak reference, so it stops once the router is gone.
    pub fn start(pool: SqlitePool, config: &ServerConfig, sessions: SessionManager) -> Arc<Self> {
        let monitor = Arc::new(Self::new(pool, config, sessions));
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let weak: Weak<Self> = Arc::downgrade(&monitor);
            let task = runtime.spawn(async move {
                let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
                loop {
                    ticker.tick().await;
                    let Some(monitor) = weak.upgrade() else { break };
                    let sample = monitor.sample();
                    push_sample(&mut locked(&monitor.history), sample);
                }
            });
            *locked(&monitor.task) = Some(task);
        }
        monitor
    }

    fn new(pool: SqlitePool, config: &ServerConfig, sessions: SessionManager) -> Self {
        let upload_dir = absolute(&config.storage.upload_dir);
        let music_dir = config.music.storage_dir.as_deref().map_or_else(|| upload_dir.join("music"), absolute);
        let cpus = std::thread::available_parallelism().map_or(1.0, |n| n.get() as f32);
        let (sent_in, sent_out) = (TRAFFIC.bytes_in.load(Ordering::Relaxed), TRAFFIC.bytes_out.load(Ordering::Relaxed));
        Self {
            pool,
            sessions,
            data_dir: upload_dir.clone(),
            upload_dir,
            music_dir,
            started: Instant::now(),
            meter: Mutex::new(Meter {
                system: System::new(),
                disks: Disks::new_with_refreshed_list(),
                pid: Pid::from_u32(std::process::id()),
                cpus,
                last_at: Instant::now(),
                last_in: sent_in,
                last_out: sent_out,
            }),
            history: Mutex::new(VecDeque::with_capacity(HISTORY_LEN)),
            task: Mutex::new(None),
        }
    }

    /// Take one measurement now.
    pub fn sample(&self) -> HealthSample {
        let now = Instant::now();
        let mut meter = locked(&self.meter);
        let pid = meter.pid;
        meter.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let (rss_bytes, cpu) = meter.system.process(pid).map_or((0, 0.0), |p| (p.memory(), p.cpu_usage()));
        meter.system.refresh_memory();
        meter.disks.refresh(true);

        let elapsed = now.duration_since(meter.last_at).as_secs_f64().max(0.001);
        let bytes_in = TRAFFIC.bytes_in.load(Ordering::Relaxed);
        let bytes_out = TRAFFIC.bytes_out.load(Ordering::Relaxed);
        let per_sec = |current: u64, before: u64| (current.saturating_sub(before) as f64 / elapsed) as u64;
        let sample = HealthSample {
            at: crate::api::invites::now_secs(),
            cpu_percent: cpu / meter.cpus,
            rss_bytes,
            system_used_bytes: meter.system.used_memory(),
            system_total_bytes: meter.system.total_memory(),
            container_limit_bytes: container_limit(),
            disk_free_bytes: free_space_for(
                meter.disks.list().iter().map(|disk| (disk.mount_point(), disk.available_space())),
                &self.data_dir,
            ),
            sessions: self.sessions.sessions.len() as u32,
            voice_users: self.sessions.voice_rooms.iter().map(|room| room.participants.len() as u32).sum(),
            bytes_in_per_sec: per_sec(bytes_in, meter.last_in),
            bytes_out_per_sec: per_sec(bytes_out, meter.last_out),
            datagrams_dropped: TRAFFIC.datagrams_dropped.load(Ordering::Relaxed),
        };
        meter.last_at = now;
        meter.last_in = bytes_in;
        meter.last_out = bytes_out;
        sample
    }

    /// The admin panel's view: the newest sample, the history and the folder sizes.
    pub async fn report(&self) -> HealthReport {
        let latest = locked(&self.history).back().cloned();
        let now = latest.unwrap_or_else(|| self.sample());
        let history: Vec<HealthSample> = locked(&self.history).iter().cloned().collect();
        let db_bytes = sqlx::query_scalar::<_, i64>(
            "SELECT (SELECT page_count FROM pragma_page_count()) * (SELECT page_size FROM pragma_page_size())",
        )
        .fetch_one(&self.pool)
        .await
        .map_or(0, |bytes| bytes.max(0) as u64);
        let (upload_dir, music_dir) = (self.upload_dir.clone(), self.music_dir.clone());
        let (uploads, music_bytes) = tokio::task::spawn_blocking(move || (dir_size(&upload_dir), dir_size(&music_dir)))
            .await
            .unwrap_or((0, 0));
        let uploads_bytes = if self.music_dir.starts_with(&self.upload_dir) { uploads.saturating_sub(music_bytes) } else { uploads };
        HealthReport {
            warnings: warnings_for(&now),
            now,
            history,
            db_bytes,
            uploads_bytes,
            music_bytes,
            uptime_secs: self.started.elapsed().as_secs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(rss_bytes: u64, container_limit_bytes: Option<u64>, system_total_bytes: u64, disk_free_bytes: u64) -> HealthSample {
        HealthSample { rss_bytes, container_limit_bytes, system_total_bytes, disk_free_bytes, ..HealthSample::default() }
    }

    #[test]
    fn health_reads_cgroup_limits() {
        assert_eq!(parse_cgroup_limit("268435456\n"), Some(268_435_456));
        assert_eq!(parse_cgroup_limit("max\n"), None);
        assert_eq!(parse_cgroup_limit(""), None);
        assert_eq!(parse_cgroup_limit("9223372036854771712\n"), None, "cgroup v1 'no limit' value");
    }

    #[test]
    fn health_history_keeps_the_last_720_samples() {
        let mut history = VecDeque::new();
        for at in 0..800 {
            push_sample(&mut history, HealthSample { at, ..HealthSample::default() });
        }
        assert_eq!(history.len(), HISTORY_LEN);
        assert_eq!(history.front().map(|s| s.at), Some(80));
        assert_eq!(history.back().map(|s| s.at), Some(799));
    }

    #[test]
    fn health_warns_about_memory_against_the_container_limit_first() {
        let gib = 1024 * 1024 * 1024;
        // 90 % of a 256 MB container limit, although the machine has plenty of memory.
        let in_container = sample(230 * 1024 * 1024, Some(256 * 1024 * 1024), 16 * gib, 10 * gib);
        assert_eq!(warnings_for(&in_container), vec!["memory-high".to_string()]);
        // 50 % of the container limit, although it is above 85 % of a small machine.
        let fine_in_container = sample(4 * gib, Some(8 * gib), 4 * gib + 1, 10 * gib);
        assert!(warnings_for(&fine_in_container).is_empty());
        // Without a container the machine's memory is the limit.
        let no_container = sample(15 * gib, None, 16 * gib, 10 * gib);
        assert_eq!(warnings_for(&no_container), vec!["memory-high".to_string()]);
    }

    #[test]
    fn health_warns_when_the_data_disk_is_nearly_full() {
        let gib = 1024 * 1024 * 1024;
        assert_eq!(warnings_for(&sample(0, None, 0, gib - 1)), vec!["disk-low".to_string()]);
        assert!(warnings_for(&sample(0, None, 0, gib)).is_empty());
    }

    #[test]
    fn health_disk_is_the_mount_that_holds_the_data_folder() {
        let mounts = [(Path::new("/"), 50), (Path::new("/data"), 7), (Path::new("/other"), 1)];
        assert_eq!(free_space_for(mounts, Path::new("/data/uploads")), 7);
        assert_eq!(free_space_for(mounts, Path::new("/home/finch")), 50);
        assert_eq!(free_space_for(mounts[1..].iter().copied(), Path::new("/home/finch")), 0, "no matching disk");
    }

    #[tokio::test]
    async fn health_sampler_records_a_sample_as_it_starts() -> anyhow::Result<()> {
        let pool = crate::db::init_db("sqlite::memory:").await?;
        let mut config = ServerConfig::default();
        config.storage.upload_dir = std::env::temp_dir().join("finch-health-unit").to_string_lossy().to_string();
        let monitor = HealthMonitor::start(pool, &config, SessionManager::new());
        for _ in 0..50 {
            if !locked(&monitor.history).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let report = monitor.report().await;
        assert!(!report.history.is_empty());
        assert!(report.now.system_total_bytes > 0);
        assert!(report.now.rss_bytes > 0);
        Ok(())
    }
}
