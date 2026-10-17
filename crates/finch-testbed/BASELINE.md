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

## Load: 50 people at once (stage 7, release build)

`cargo test -p finch-testbed --test load_50 --release -- --ignored --nocapture` (lines `LOAD ...`).
50 clients in one voice room, 6 of them talking (50 packets/s each), 5 chatting (1 message/s each), 2 screen shares
(about 30 pictures/s, 3 fragments each) with 6 viewers each, 20 s. Server and all clients share one process on one PC,
over localhost, synthetic audio and video payloads (no real Opus or H.264 work).

| Metric | Required | Measured (2026-10-16) | Status |
|---|---|---|---|
| Sign-in of 50 clients | info | 1.4 s | info |
| Voice packets delivered (6 speakers x 49 listeners) | >= 97 % | 294294 / 294294 = 100.00 % | pass |
| Video packets delivered (2 shares x 6 viewers) | >= 90 % | 18036 / 18036 = 100.00 % | pass |
| Chat messages delivered (5 chatters x 50 receivers) | >= 99 % | 5000 / 5000 | pass |
| Chat latency p50 / p95 / max | p95 < 500 ms | 18 / 62 / 64 ms | pass |
| Connections dropped | 0 | 0 | pass |
| CPU, whole process (server + 50 clients) | info | avg 63 %, max 188 % of one core | info |
| Memory, whole process | info | 47 MB | info |

Not measured here: real microphones and decoders on 50 machines, internet latency, TLS handshakes from 50 different addresses.

Release preparation rerun, 2026-10-16 (1.0.0): 50 sign-ins in 1.0 s;
voice 294294/294294 (100%), video 18036/18036 (100%), chat 5000/5000;
chat p50/p95/max 20/63/68 ms; zero disconnects; whole-process RSS 48 MB;
CPU average/max 109%/272% of one core. Same localhost/synthetic-payload limits apply.

Final jitter correction (2026-10-16): signed timestamp deltas for reordered
packets and preserving missing PLC slots unless a real backlog is trimmed.
Netem 5% loss / 0–60 ms jitter: probe latencies 96.44 / 96.42 / 96.44 ms,
maximum buffer 5 packets, longest silent run 0 frames, 268/284 received.
The three probe matching checks and the 200 ms / six-packet limits are unchanged.

## Voice under loss and jitter: made repeatable (stage 7)

The test failed in about 1 of 3 runs because the loss pattern still depended on connection set-up timing and on how
the two directions interleaved. The proxy now starts impairing after both clients are in the room and uses one random
sequence per direction. Result: 8 of 8 runs identical in delivery (268 of 284 packets), all passing.

## Native WebRTC 1.1.0 candidate (2026-10-17)
Original C++ WebRTC APM M131/standalone2.1 + existing RNNoise/nnnoiseless0.5.2. No own compressor or Sonora in production. AGC2: headroom6dB, adaptive gain0…12dB, rate6dB/s, noise cap−50dBFS, initial0dB; fixed digital gain follows manual trim, final constant−1dB headroom.
A/B uses 15s repeated fixtures, identical common voiced windows in the last5s after10s adaptation. Nominal input is whole-signal RMS; output columns are voiced-window RMS, so their absolute values differ. No output/VAD-selected windows are discarded. Source clipping is explicitly counted before processing.
| Fixture | Nominal input dBFS | Enhancement off dBFS | On dBFS | Change dB | Source clipped samples |
|---|---:|---:|---:|---:|---:|
| speech_1 | -38 | -34.79 | -23.83 | +10.95 | 0 |
| speech_1 | -26 | -22.83 | -23.82 | -0.99 | 0 |
| speech_1 | -12 | -9.55 | -10.66 | -1.11 | 8955 |
| speech_2 | -38 | -36.54 | -25.54 | +11.00 | 0 |
| speech_2 | -26 | -24.54 | -23.52 | +1.02 | 0 |
| speech_2 | -12 | -10.95 | -11.99 | -1.04 | 8118 |
| speech_3 | -38 | -36.73 | -25.73 | +11.00 | 0 |
| speech_3 | -26 | -24.74 | -23.90 | +0.83 | 0 |
| speech_3 | -12 | -11.02 | -12.10 | -1.07 | 6441 |

All three quiet sources gain10.95–11.00dB; inter-source level spread is reduced by more than12dB. Loud sources are not boosted. Peak≤−1dBFS in steady enabled mode; full-bypass peak≤0dBFS, transitions20ms. Already clipped input is not claimed to be restored.

Isolated unchanged AEC/NS gates: ERLE74.4dB, double-talk−25.8 versus near−23.3dBFS; full default-on ERLE75.4dB, double-talk−26.8dBFS. White/pink NS improvements+22.2/+15.7dB, segmental SNR preserved; babble measured only. Default-on onset starts40/20/60ms before Energy detector for the three voices; pink−35 and white−45dBFS produce no noise-only packets. Headless localhost latency76.4ms (excludes physical device buffers).

New least-squares gain-calibrated residual SNR is invariant to a constant output gain (0.25x/4x control within0.1dB), uses the same voiced windows and fixed calibrated RNNoise lag480samples, and includes signal distortion in residual. It is diagnostic fidelity, not a human MOS score. On/off stationary-noise results differ by at most0.03dB; AGC is not credited with extra noise suppression. Separate-pause SNR is used only for the AEC/NS component with AGC off; full default-on is checked separately.

Artifacts: target/voice-enhancement-out (input/off/on WAVs, levels.json, stationary.json, pause/transient WAVs). Commands: cargo test -p finch-testbed --test voice_enhancement -- --test-threads=1 --nocapture; cargo test -p finch-testbed --test voice_quality -- --test-threads=1 --nocapture. Debug gain-adapter60s signal cost1.827s with zero Rust allocations; native C++ operator-new counter also remains zero after warm-up (does not instrument legacy C malloc).

Regression fixes exposed by these cases: bound the Earshot analysis after FFT resampling overshoot; coalesce manual gain changes before native processing and propagate a full native runtime queue as an error. Physical microphones, listening on two PCs and NAT remain owner acceptance, not inferred from synthesized-fixture metrics.

Repeatability: three sequential idle-host latency runs each76.4ms. Pause noise first/last full second: pink -66.14/-92.71dBFS; white -73.35/-95.58dBFS. Windows/MSVC debug host, candidate based on6d14ecd plus quality regression fixes; physical-device latency is excluded.
