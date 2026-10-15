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
| AEC (Sonora AEC3), echo 40 ms / −6 dB, speech: ERLE after 2 s | ≥ 20 dB | 77.8 dB (was 0.8 dB with the scalar filter) | pass | 2026-10-16 | 0b7335e |
| AEC double-talk, speech: near-end level kept | ≥ near − 6 dB | −25.8 dBFS (near −23.3, loss 2.5 dB) | pass | 2026-10-16 | 0b7335e |
| VAD (Earshot after RNNoise), pink noise −35 dBFS: speaking indicator | never lit | 0 (was 2) | pass | 2026-10-16 | AUD-050 |
| VAD, pink noise −35 dBFS: non-silent packets | 0 | 0 (was 104) | pass | 2026-10-16 | AUD-050 |
| VAD, same run: real speech afterwards | indicator lit, packets delivered | lit, 140 packets | pass | 2026-10-16 | AUD-050 |
| VAD, white noise −45 dBFS (mic hiss): packets | 0 | 0 | pass | 2026-10-16 | AUD-050 |
| VAD, white noise −35 dBFS (far above a real mic floor): speech-positive frames | info | 3.3% (5 of 150, one run of 5) | known limit | 2026-10-16 | probe |
| VAD onset, 3 voices: stream start vs loudness-threshold detection | ≤ 0 ms later | −40 / −20 / −60 ms (classifier lags 1–3 frames, pre-roll 4 frames) | pass | 2026-10-16 | AUD-050 |
| Network 5% loss + 0–60 ms jitter: latency (3 runs, VAD off) | ≤ 200 ms | 96 / 136 / 76 ms | pass | 2026-10-15 | after AUD-022 |
| Network: packets delivered by the network | — | 254–259 of 265 (~97%) | info | 2026-10-15 | after AUD-022 |
| Network: jitter buffer depth | ≤ 6 packets | 6 / 6 / 6 | pass | 2026-10-15 | after AUD-022 |
| Network: jitter-buffer loss rate | — | 0.4–2.4% | info | 2026-10-15 | after AUD-022 |
| Network: longest run of silent frames during speech | ≤ 3 | 0 / 0 / 0 | pass | 2026-10-15 | after AUD-022 |
| Screen share 1080p, per frame (release, this PC): convert + encode | info | 62 ms before, 31 ms after (own parallel I420 converter); capture 23 ms runs in parallel | improved | 2026-10-16 | screen FPS |
| Screen share 1080p, viewer FPS in the app (two clients, one PC) | ≥ 25 | 16 before, 30 after | pass | 2026-10-16 | screen FPS |
| End-to-end latency, Energy VAD with pre-roll (after AUD-050 pre-roll burst) | ≤ 120 ms | 116–136 ms before the catch-up fix, 76.4 ms after (5 runs) | fixed | 2026-10-16 | jitter catch-up |
| (before AUD-022 fix) jitter buffer depth / loss | — | 20–68 packets / 17–35% | historical | 2026-10-15 | 27dcb0f |

Network results vary between runs (real-time scheduling); three runs are recorded.

Speech fixtures are TTS-synthesized (see `fixtures/ATTRIBUTION.md`); real recordings may score slightly lower.
