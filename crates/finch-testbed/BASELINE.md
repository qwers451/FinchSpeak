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
| NS, babble (3 voices): SNR improvement | ≥ 10 dB | +0.0 dB (6.0 → 6.0) | **fail — AUD-049** | 2026-10-15 | cc0476d |
| NS, babble: segmental SNR in → out | out ≥ in | 5.9 → 5.2 | **fail — AUD-049** | 2026-10-15 | cc0476d |
| AEC, echo 40 ms / −6 dB: ERLE after 2 s | ≥ 20 dB | 0.8 dB | **fail — AUD-021** | 2026-10-15 | cc0476d |
| AEC double-talk: near-end level kept | ≥ near − 6 dB | −22.1 dBFS (near −23.3) | pass | 2026-10-15 | cc0476d |
| VAD on pink noise −35 dBFS: speaking indicator | never lit | lit 2× | **fail — AUD-050** | 2026-10-15 | cc0476d |
| VAD on pink noise −35 dBFS: non-silent packets | 0 | 104 | **fail — AUD-050** | 2026-10-15 | cc0476d |

Speech fixtures are TTS-synthesized (see `fixtures/ATTRIBUTION.md`); real recordings may score slightly lower.
