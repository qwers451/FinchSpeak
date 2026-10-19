//! Opt-in, bounded audio telemetry. Never stores PCM, packet payloads or account data.
use std::collections::VecDeque;
use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Mutex};
use std::time::Instant;
use serde::Serialize;

const HISTORY_LIMIT: usize = 120;

#[derive(Clone, Default, Serialize)]
pub struct DeviceConfig {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

#[derive(Clone, Copy)]
pub enum Metric {
    CaptureDroppedSamples, OutputUnderflowSamples, OutputUnderflowCallbacks,
    ReferenceDroppedSamples, MonitorDroppedFrames, MonitorEmptyFrames, TestActivationRejectedFrames,
    CaptureHighWaterSamples, CaptureBatchMaxFrames, CaptureBatchMaxUs, PlaybackRefillMaxGapUs, TestPttClosedFrames,
}
const METRIC_NAMES: [&str; 12] = [
    "capture_dropped_samples", "output_underflow_samples", "output_underflow_callbacks",
    "reference_dropped_samples", "monitor_dropped_frames", "monitor_empty_frames", "test_activation_rejected_frames_before_preroll",
    "capture_high_water_samples", "capture_batch_max_frames", "capture_batch_max_us", "playback_refill_max_gap_us", "test_ptt_closed_frames",
];

struct CallbackStats {
    count: AtomicU64, frames: AtomicU64, min: AtomicU64, max: AtomicU64, gap: AtomicU64,
}
impl Default for CallbackStats {
    fn default() -> Self {
        Self { count: AtomicU64::new(0), frames: AtomicU64::new(0), min: AtomicU64::new(u64::MAX),
            max: AtomicU64::new(0), gap: AtomicU64::new(0) }
    }
}
impl CallbackStats {
    fn reset(&self) {
        self.count.store(0, Ordering::Relaxed); self.frames.store(0, Ordering::Relaxed);
        self.min.store(u64::MAX, Ordering::Relaxed); self.max.store(0, Ordering::Relaxed); self.gap.store(0, Ordering::Relaxed);
    }
    fn record(&self, frames: usize, gap_us: u64) {
        self.count.fetch_add(1, Ordering::Relaxed); self.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.min.fetch_min(frames as u64, Ordering::Relaxed); self.max.fetch_max(frames as u64, Ordering::Relaxed);
        self.gap.fetch_max(gap_us, Ordering::Relaxed);
    }
    fn snapshot(&self) -> serde_json::Value {
        let count = self.count.load(Ordering::Relaxed);
        serde_json::json!({ "callbacks": count, "total_frames": self.frames.load(Ordering::Relaxed),
            "min_frames": if count == 0 { 0 } else { self.min.load(Ordering::Relaxed) },
            "max_frames": self.max.load(Ordering::Relaxed), "max_gap_us": self.gap.load(Ordering::Relaxed) })
    }
}

#[derive(Default)]
struct History {
    started: Option<Instant>, last_sample_ms: u64,
    stopped_ms: Option<u64>,
    samples: VecDeque<serde_json::Value>, stream: StreamContext,
    mode: String, muted: bool, deafened: bool,
}

#[derive(Clone, Default, Serialize)]
struct StreamContext {
    generation: u64, input: Option<DeviceConfig>, output: Option<DeviceConfig>,
    input_active: bool, output_active: bool,
}

pub struct AudioDiagnostics {
    enabled: AtomicBool,
    epoch: AtomicU64, writers: AtomicU64, control: Mutex<()>,
    input: CallbackStats, output: CallbackStats,
    metrics: [AtomicU64; 12], dsp_histogram: [AtomicU64; 32],
    dsp_count: AtomicU64, dsp_max_us: AtomicU64,
    history: Mutex<History>,
    stream: Mutex<StreamContext>,
}
impl Default for AudioDiagnostics {
    fn default() -> Self {
        Self { enabled: AtomicBool::new(false), epoch: AtomicU64::new(0), writers: AtomicU64::new(0), control: Mutex::new(()),
            input: CallbackStats::default(), output: CallbackStats::default(),
            metrics: std::array::from_fn(|_| AtomicU64::new(0)), dsp_histogram: std::array::from_fn(|_| AtomicU64::new(0)),
            dsp_count: AtomicU64::new(0), dsp_max_us: AtomicU64::new(0), history: Mutex::new(History::default()),
            stream: Mutex::new(StreamContext::default()) }
    }
}
impl AudioDiagnostics {
    pub fn enabled(&self) -> bool { self.enabled.load(Ordering::Relaxed) }
    pub fn epoch(&self) -> u64 {
        let epoch = self.epoch.load(Ordering::SeqCst);
        if self.enabled.load(Ordering::SeqCst) { epoch } else { 0 }
    }
    // Control operations may wait; the real-time path never waits or locks.
    fn pause_writers(&self) {
        self.enabled.store(false, Ordering::SeqCst);
        while self.writers.load(Ordering::SeqCst) != 0 { std::thread::yield_now(); }
    }
    fn record(&self, epoch: u64, update: impl FnOnce()) {
        if epoch == 0 { return; }
        self.writers.fetch_add(1, Ordering::SeqCst);
        if self.enabled.load(Ordering::SeqCst) && self.epoch.load(Ordering::SeqCst) == epoch { update(); }
        self.writers.fetch_sub(1, Ordering::SeqCst);
    }
    pub fn start(&self) {
        let _control = self.control.lock().unwrap_or_else(|e| e.into_inner());
        self.pause_writers();
        self.input.reset(); self.output.reset();
        for value in self.metrics.iter().chain(self.dsp_histogram.iter()) { value.store(0, Ordering::Relaxed); }
        self.dsp_count.store(0, Ordering::Relaxed); self.dsp_max_us.store(0, Ordering::Relaxed);
        let stream = self.stream.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        history.stream = stream;
        history.samples.clear(); history.started = Some(Instant::now()); history.last_sample_ms = 0; history.stopped_ms = None;
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.enabled.store(true, Ordering::SeqCst);
    }
    pub fn stop(&self) {
        let _control = self.control.lock().unwrap_or_else(|e| e.into_inner());
        self.pause_writers();
        let stream = self.stream.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        if history.started.is_some() && history.stopped_ms.is_none() {
            history.stream = stream;
            let elapsed = history.started.unwrap().elapsed().as_millis() as u64;
            self.sample(&mut history, elapsed);
            history.stopped_ms = Some(elapsed);
        }
    }
    pub fn configure(&self, input: bool, config: DeviceConfig) {
        let mut stream = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        if input { stream.input = Some(config); } else { stream.output = Some(config); }
    }
    pub fn stream_begin(&self) {
        let mut stream = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        *stream = StreamContext { generation: stream.generation + 1, ..Default::default() };
    }
    pub fn stream_active(&self, input: bool, active: bool) {
        let mut stream = self.stream.lock().unwrap_or_else(|e| e.into_inner());
        if input { stream.input_active = active; } else { stream.output_active = active; }
    }
    /// Callback-safe: only bounded atomics; caller provides timing from its local clock.
    pub fn callback(&self, input: bool, frames: usize, gap_us: u64) {
        self.callback_at(self.epoch(), input, frames, gap_us);
    }
    pub fn callback_at(&self, epoch: u64, input: bool, frames: usize, gap_us: u64) {
        self.record(epoch, || { if input { &self.input } else { &self.output }.record(frames, gap_us); });
    }
    pub fn add(&self, metric: Metric, value: u64) {
        self.add_at(self.epoch(), metric, value);
    }
    pub fn add_at(&self, epoch: u64, metric: Metric, value: u64) {
        self.record(epoch, || { self.metrics[metric as usize].fetch_add(value, Ordering::Relaxed); });
    }
    pub fn maximum(&self, metric: Metric, value: u64) {
        self.maximum_at(self.epoch(), metric, value);
    }
    pub fn maximum_at(&self, epoch: u64, metric: Metric, value: u64) {
        self.record(epoch, || { self.metrics[metric as usize].fetch_max(value, Ordering::Relaxed); });
    }
    pub fn dsp_frame(&self, us: u64) {
        self.dsp_frame_at(self.epoch(), us);
    }
    pub fn dsp_frame_at(&self, epoch: u64, us: u64) {
        self.record(epoch, || {
        let bucket = if us <= 1 { 0 } else { (64 - (us - 1).leading_zeros() as usize).min(31) };
        self.dsp_histogram[bucket].fetch_add(1, Ordering::Relaxed);
        self.dsp_count.fetch_add(1, Ordering::Relaxed); self.dsp_max_us.fetch_max(us, Ordering::Relaxed);
        });
    }
    fn counters(&self) -> serde_json::Value {
        let mut metrics = serde_json::Map::new();
        for (index, name) in METRIC_NAMES.iter().enumerate() { metrics.insert((*name).into(), self.metrics[index].load(Ordering::Relaxed).into()); }
        let count = self.dsp_count.load(Ordering::Relaxed);
        let threshold = count.saturating_mul(95).div_ceil(100);
        let mut cumulative = 0;
        let mut p95 = 0u64;
        for (index, bucket) in self.dsp_histogram.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if count > 0 && cumulative >= threshold { p95 = 1u64 << index; break; }
        }
        serde_json::json!({ "input": self.input.snapshot(), "output": self.output.snapshot(), "metrics": metrics,
            "dsp": { "frames": count, "max_us": self.dsp_max_us.load(Ordering::Relaxed), "p95_upper_bound_us": p95 } })
    }
    pub fn tick(&self, mode: &str, muted: bool, deafened: bool) {
        if !self.enabled() { return; }
        {
            let Ok(history) = self.history.try_lock() else { return; };
            let elapsed = history.started.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0);
            if elapsed.saturating_sub(history.last_sample_ms) < 1000
                && history.mode == mode && history.muted == muted && history.deafened == deafened { return; }
        }
        let stream = self.stream.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Ok(mut history) = self.history.try_lock() else { return; };
        if !self.enabled() { return; }
        history.stream = stream;
        let elapsed = history.started.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0);
        if history.mode != mode { history.mode = mode.to_string(); }
        history.muted = muted; history.deafened = deafened;
        if elapsed.saturating_sub(history.last_sample_ms) >= 1000 { self.sample(&mut history, elapsed); }
    }
    fn sample(&self, history: &mut History, elapsed: u64) {
        if history.samples.len() == HISTORY_LIMIT { history.samples.pop_front(); }
        history.samples.push_back(serde_json::json!({ "elapsed_ms": elapsed, "counters": self.counters(),
            "stream": history.stream,
            "mode": history.mode, "muted": history.muted, "deafened": history.deafened }));
        history.last_sample_ms = elapsed;
    }
    pub fn report(&self) -> Result<String, serde_json::Error> {
        let history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let report = serde_json::json!({ "schema_version": 1, "client_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "enabled": self.enabled(),
            "diagnostic_epoch": self.epoch.load(Ordering::SeqCst),
            "contains_audio": false, "stream": history.stream,
            "elapsed_ms": history.stopped_ms.unwrap_or_else(|| history.started.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0)),
            "counter_semantics": "cumulative across stream generations since diagnostics start; lock-free counters are approximate during capture; activation rejects include PTT and may be restored by preroll",
            "samples": history.samples, "current": self.counters() });
        drop(history);
        serde_json::to_string_pretty(&report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_are_opt_in_and_restart_clears_counters() {
        let d = AudioDiagnostics::default();
        d.callback(true, 480, 10_000); d.add(Metric::CaptureDroppedSamples, 4);
        assert_eq!(d.counters()["input"]["callbacks"], 0);
        d.start(); d.callback(true, 480, 10_000); d.add(Metric::CaptureDroppedSamples, 4);
        assert_eq!(d.counters()["metrics"]["capture_dropped_samples"], 4);
        d.stop(); d.callback(true, 960, 20_000);
        assert_eq!(d.counters()["input"]["callbacks"], 1);
        d.start(); assert_eq!(d.counters()["input"]["callbacks"], 0);
    }
    #[test]
    fn diagnostics_measure_callbacks_dsp_and_bound_history() {
        let d = AudioDiagnostics::default(); d.start();
        d.callback(false, 480, 10_000); d.callback(false, 4800, 100_000);
        for _ in 0..95 { d.dsp_frame(1500); } for _ in 0..5 { d.dsp_frame(13_000); }
        d.maximum(Metric::CaptureBatchMaxUs, 70_000); d.maximum(Metric::CaptureBatchMaxUs, 100);
        let mut history = d.history.lock().unwrap();
        for i in 1..=200 { d.sample(&mut history, i * 1000); }
        drop(history);
        let report: serde_json::Value = serde_json::from_str(&d.report().unwrap()).unwrap();
        assert_eq!(report["samples"].as_array().unwrap().len(), 120);
        assert_eq!(report["current"]["output"]["min_frames"], 480);
        assert_eq!(report["current"]["output"]["max_frames"], 4800);
        assert_eq!(report["current"]["dsp"]["p95_upper_bound_us"], 2048);
        assert_eq!(report["current"]["dsp"]["max_us"], 13_000);
        assert_eq!(report["current"]["metrics"]["capture_batch_max_us"], 70_000);
        assert_eq!(report["contains_audio"], false);
    }

    #[test]
    fn stopped_report_is_stable_and_has_no_audio_or_session_payload_fields() {
        let d = AudioDiagnostics::default(); d.start();
        d.configure(true, DeviceConfig { name: "Test mic".into(), sample_rate: 48_000, channels: 1, sample_format: "F32".into() });
        d.tick("microphone-test-recording", false, false); d.stop();
        let first = d.report().unwrap();
        d.callback(true, 960, 20_000); d.add(Metric::CaptureDroppedSamples, 500);
        d.stream_begin(); d.configure(false, DeviceConfig { name: "Another output".into(), ..Default::default() });
        assert_eq!(first, d.report().unwrap());
        let report: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(report["samples"][0]["mode"], "microphone-test-recording");
        for forbidden in ["pcm", "audio_samples", "token", "password", "server_url", "user_id", "messages"] {
            assert!(!first.contains(&format!("\"{forbidden}\"")));
        }
    }

    #[test]
    fn old_epoch_cannot_pollute_reset_counters_and_device_status_is_explicit() {
        let d = AudioDiagnostics::default();
        d.stream_begin(); d.configure(true, DeviceConfig { name: "Configured only".into(), ..Default::default() });
        d.start(); let old_epoch = d.epoch();
        d.callback_at(old_epoch, true, 480, 10_000);
        d.start();
        d.callback_at(old_epoch, true, 999, 999_999);
        d.dsp_frame_at(old_epoch, 999_999);
        d.maximum_at(old_epoch, Metric::CaptureBatchMaxUs, 999_999);
        assert_eq!(d.counters()["input"]["callbacks"], 0);
        assert_eq!(d.counters()["dsp"]["frames"], 0);
        assert_eq!(d.counters()["metrics"]["capture_batch_max_us"], 0);
        let report: serde_json::Value = serde_json::from_str(&d.report().unwrap()).unwrap();
        assert_eq!(report["stream"]["input_active"], false);
        d.stream_active(true, true); d.tick("voice", false, false); d.stop();
        let report: serde_json::Value = serde_json::from_str(&d.report().unwrap()).unwrap();
        assert_eq!(report["stream"]["input_active"], true);
        assert_eq!(report["stream"]["generation"], 1);
        d.stream_begin(); d.start();
        let report: serde_json::Value = serde_json::from_str(&d.report().unwrap()).unwrap();
        assert!(report["stream"]["input"].is_null());
        assert_eq!(report["stream"]["input_active"], false);
    }
}
