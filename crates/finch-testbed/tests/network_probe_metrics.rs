//! CPU-heavy correlation checks run separately from wall-clock audio streaming.
use finch_testbed::audio::chirp;
const SR: usize = 48_000;

#[path = "support/latency_probes.rs"]
mod latency_probes;
use latency_probes::probe_latencies_of;

/// Measure each probe against the receiver's wall-clock timeline, not the previous probe.
fn probe_latencies(input: &[f32], output: &[f32], starts: &[usize]) -> anyhow::Result<Vec<usize>> {
    probe_latencies_of(input, output, starts, SR / 10)
}

#[test]
fn latency_probes_reject_late_second_probe_after_timely_first() {
    let starts = [SR / 5, 2 * SR + SR / 5];
    let mut input = vec![0.0; 3 * SR];
    let probe = chirp(SR / 10, 300.0, 3_000.0);
    let mut output = vec![0.0; 4 * SR];
    for (&start, delay) in starts.iter().zip([SR / 10, 3 * SR / 10]) {
        input[start..start + probe.len()].copy_from_slice(&probe);
        output[start + delay..start + delay + probe.len()].copy_from_slice(&probe);
    }
    let error = probe_latencies(&input, &output, &starts).expect_err("later latency must be checked");
    assert!(error.to_string().contains("latency 300 ms > 200 ms"), "{error}");
}

#[test]
fn latency_probes_reject_missing_second_probe() {
    let starts = [SR / 5, 2 * SR + SR / 5];
    let mut input = vec![0.0; 3 * SR];
    let probe = chirp(SR / 10, 300.0, 3_000.0);
    let mut output = vec![0.0; 4 * SR];
    for &start in &starts {
        input[start..start + probe.len()].copy_from_slice(&probe);
    }
    output[starts[0] + SR / 10..starts[0] + SR / 10 + probe.len()].copy_from_slice(&probe);
    let error = probe_latencies(&input, &output, &starts).expect_err("missing later probe must fail");
    assert!(error.to_string().contains("not found"), "{error}");
}

#[test]
fn latency_probes_measure_independent_delays() -> anyhow::Result<()> {
    let starts = [SR / 5, 2 * SR + SR / 5, 4 * SR + SR / 5];
    let mut input = vec![0.0; 5 * SR];
    let probe = chirp(SR / 10, 300.0, 3_000.0);
    let mut output = vec![0.0; 6 * SR];
    for (&start, delay) in starts.iter().zip([4_800, 7_200, 9_600]) {
        input[start..start + probe.len()].copy_from_slice(&probe);
        output[start + delay..start + delay + probe.len()].copy_from_slice(&probe);
    }
    assert_eq!(probe_latencies(&input, &output, &starts)?, [4_800, 7_200, 9_600]);
    Ok(())
}

