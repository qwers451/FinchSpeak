# Voice quality baseline

Measured by `cargo test -p finch-testbed --test voice_quality -- --include-ignored --nocapture`
(lines starting with `BASELINE`). Thresholds come from the spec and are not lowered without the owner's approval.
Debug WAVs of the last run: `target/testbed-out/`.

| Metric | Threshold | Measured | Status | Date | Commit |
|---|---|---|---|---|---|
| End-to-end latency A → SFU → B (localhost, without device buffers) | ≤ 120 ms | 56.4 ms | pass | 2026-10-15 | cc0476d |
| NS, white noise, input SNR 5 dB: SNR improvement | ≥ 10 dB | +22.2 dB (5.0 → 27.3) | pass | 2026-10-15 | cc0476d |
| NS, white noise: segmental SNR in → out | out ≥ in | 5.7 → 26.9 | pass | 2026-10-15 | cc0476d |
| NS, pink noise: SNR improvement | ≥ 10 dB | +15.7 dB (5.2 → 20.9) | pass | 2026-10-15 | cc0476d |
| NS, pink noise: segmental SNR in → out | out ≥ in | 5.8 → 20.4 | pass | 2026-10-15 | cc0476d |
| NS, babble (3 voices): SNR improvement | measured only (owner decision, AUD-049) | +0.0 dB (6.0 → 6.0) | info | 2026-10-15 | cc0476d |
| NS, babble: segmental SNR in → out | measured only | 5.9 → 5.2 | info | 2026-10-15 | cc0476d |
| AEC, echo 40 ms / −6 dB: ERLE after 2 s | ≥ 20 dB | 0.8 dB | **fail — AUD-021** | 2026-10-15 | cc0476d |
| AEC double-talk: near-end level kept | ≥ near − 6 dB | −22.1 dBFS (near −23.3) | pass | 2026-10-15 | cc0476d |
| VAD on pink noise −35 dBFS: speaking indicator | never lit | lit 2× | **fail — AUD-050** | 2026-10-15 | cc0476d |
| VAD on pink noise −35 dBFS: non-silent packets | 0 | 104 | **fail — AUD-050** | 2026-10-15 | cc0476d |
| Network 5% loss + 0–60 ms jitter: latency (3 runs, VAD off) | ≤ 200 ms | 96 / 136 / 76 ms | pass | 2026-10-15 | after AUD-022 |
| Network: packets delivered by the network | — | 254–259 of 265 (~97%) | info | 2026-10-15 | after AUD-022 |
| Network: jitter buffer depth | ≤ 6 packets | 6 / 6 / 6 | pass | 2026-10-15 | after AUD-022 |
| Network: jitter-buffer loss rate | — | 0.4–2.4% | info | 2026-10-15 | after AUD-022 |
| Network: longest run of silent frames during speech | ≤ 3 | 0 / 0 / 0 | pass | 2026-10-15 | after AUD-022 |
| (before AUD-022 fix) jitter buffer depth / loss | — | 20–68 packets / 17–35% | historical | 2026-10-15 | 27dcb0f |

Network results vary between runs (real-time scheduling); three runs are recorded.

Speech fixtures are TTS-synthesized (see `fixtures/ATTRIBUTION.md`); real recordings may score slightly lower.
