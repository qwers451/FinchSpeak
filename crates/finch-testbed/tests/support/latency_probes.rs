use finch_testbed::metrics::delay_samples;
const SR: usize = 48_000;

/// Measure each probe against the receiver timeline using an explicit reference length.
pub fn probe_latencies_of(input: &[f32], output: &[f32], starts: &[usize], len: usize) -> anyhow::Result<Vec<usize>> {
    starts
        .iter()
        .map(|&start| {
            let reference = &input[start..start + len];
            let end = (start + reference.len() + SR / 2).min(output.len());
            let delay = delay_samples(reference, &output[start..end], SR / 2)
                .ok_or_else(|| anyhow::anyhow!("probe at sample {start} not found in receiver output"))?;
            anyhow::ensure!(
                delay <= SR / 5,
                "probe at sample {start}: latency {} ms > 200 ms",
                delay * 1000 / SR
            );
            Ok(delay)
        })
        .collect()
}

