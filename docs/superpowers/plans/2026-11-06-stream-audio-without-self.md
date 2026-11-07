# Stream Audio Without Your Own Voice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Screen-share audio carries everything the computer plays **except FinchSpeak itself**, so viewers no longer hear their own voice coming back through the stream. When sharing one window, the user can send only that window's sound.

**Architecture:** On Windows 10 2004+ (build ≥ 19041) the capturer uses WASAPI *process loopback* (`ActivateAudioInterfaceAsync` on `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`) with `EXCLUDE_TARGET_PROCESS_TREE` and our own PID. The captured 48 kHz stereo f32 samples go into the existing ring buffer → `ResamplerConverter` → `ScreenAudioEncoder` pipeline unchanged. Older Windows keeps today's device loopback and the UI says so honestly.

**Tech Stack:** Rust, `windows` 0.62 (already in `Cargo.lock`), cpal 0.15 (fallback), audiopus, SolidJS.

**Spec:** `docs/UX-AUDIT-2026-11.md` — item П15.

## Global Constraints

- Same build/test/commit rules as `2026-11-06-sign-in-and-startup.md` (Global Constraints), commit prefix `fix(screen-audio): …` / `feat(screen-audio): …`.
- No mutexes and no allocations inside the capture loop's per-packet path (copy into the pre-allocated ring buffer only), same rule as the cpal callbacks.
- The capture thread never panics: every COM/WASAPI `HRESULT` error becomes `LoopbackAudioError`.
- Requested capture format is fixed: `WAVE_FORMAT_IEEE_FLOAT`, 48 000 Hz, 2 channels, 32 bit, block align 8 (process loopback does not support `GetMixFormat`).
- **Model note for the controller:** Task 2 is raw COM/WASAPI code. If a Haiku worker fails its review twice, re-dispatch that task on Sonnet.

## Review Focus

1. Windows older than build 19041 → sharing with sound still works through the old device loopback, and the picker shows the old warning (Task 3: `old_windows_falls_back_to_device_loopback`).
2. Nothing is playing (silence) → process loopback delivers no packets for long stretches; the forwarder must not treat that as an error or stop (Task 2: `silence_produces_no_error`, manual).
3. FinchSpeak output device ≠ Windows default device → exclusion must still work because it is by process, not by device (manual check, Task 4).
4. Stopping the share while the capture thread is waiting on the event → thread exits within 200 ms, no leaked COM objects (Task 2: `stop_joins_capture_thread_quickly`).
5. Sharing a window whose process exits mid-stream (window mode, Task 5) → audio stops quietly, picture continues until the existing window-closed handling ends the share.

---

### Task 1: Split the capturer into "source" and "pipeline"

**Files:**
- Modify: `crates/finch-screen/src/audio_loopback.rs:360-500` (`ScreenAudioCapturer::start`)
- Test: same file, `#[cfg(test)]`

**Interfaces:**
- Produces:
  ```rust
  /// Where screen audio comes from.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum AudioSourceKind {
      /// Everything the computer plays except this process (Windows 10 2004+).
      AllExceptSelf,
      /// Only this process tree (sharing one window).
      OnlyProcess(u32),
      /// The whole output device (old behaviour, fallback).
      Device,
  }
  ```
  New field `pub source: AudioSourceKind` in `ScreenAudioConfig` (default `AllExceptSelf`).
  New private fn `spawn_encoder_thread(config: ScreenAudioConfig, source_rate: u32, source_channels: usize, cons: <ringbuf consumer>, running: Arc<AtomicBool>) -> Result<(JoinHandle<()>, Receiver<MediaPacket>), LoopbackAudioError>` — the body of today's thread (lines ~440-497), moved, unchanged.
  New pub field on the capturer: `pub fn source(&self) -> &AudioSourceKind` — the source actually in use after fallback.
- Behaviour after this task is identical (still device loopback for every kind).

- [ ] **Step 1: Write failing test** `default_config_asks_for_all_except_self` (`ScreenAudioConfig::default().source == AudioSourceKind::AllExceptSelf`).
- [ ] **Step 2: Run** `cargo test -p finch-screen default_config` → FAIL.
- [ ] **Step 3: Implement** the enum, the field, the extraction of `spawn_encoder_thread`; the cpal path becomes `fn start_device_capture(...)`.
- [ ] **Step 4: Run** `cargo test -p finch-screen` → PASS (all existing loopback tests still pass).
- [ ] **Step 5: Commit** `refactor(screen-audio): separate the capture source from the encoder pipeline`.

### Task 2: WASAPI process loopback capture (Windows)

**Files:**
- Create: `crates/finch-screen/src/process_loopback.rs` (`#[cfg(windows)]`)
- Modify: `crates/finch-screen/Cargo.toml`:
  ```toml
  [target.'cfg(windows)'.dependencies]
  windows = { version = "0.62", features = ["implement", "Win32_Foundation", "Win32_Media_Audio", "Win32_System_Com", "Win32_System_Com_StructuredStorage", "Win32_System_Threading", "Win32_System_Variant"] }
  windows-core = "0.62"
  ```
- Modify: `crates/finch-screen/src/lib.rs` (`#[cfg(windows)] mod process_loopback;`)

**Interfaces:**
- Produces:
  ```rust
  pub struct ProcessLoopback { /* running flag, thread handle */ }
  impl ProcessLoopback {
      /// `include == false`: everything except `pid`'s tree; `true`: only `pid`'s tree.
      /// `on_samples` receives interleaved stereo f32 at 48 kHz.
      pub fn start(pid: u32, include: bool, on_samples: impl FnMut(&[f32]) + Send + 'static) -> Result<Self, LoopbackAudioError>;
      pub fn stop(&mut self);            // also called from Drop
  }
  pub fn process_loopback_supported() -> bool; // RtlGetVersion build >= 19041
  ```
  New `LoopbackAudioError::Wasapi(String)` variant (message includes the `HRESULT`).
- Approach (fixed by the Windows API, follow Microsoft's "ApplicationLoopback" sample):
  1. On the capture thread: `CoInitializeEx(None, COINIT_MULTITHREADED)`.
  2. Build `AUDIOCLIENT_ACTIVATION_PARAMS { ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, ProcessLoopbackParams: { TargetProcessId: pid, ProcessLoopbackMode: if include { PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE } else { PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE } } }`, wrap it in a `PROPVARIANT` of type `VT_BLOB`.
  3. `ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&prop), &handler)` where `handler` is a `#[implement(IActivateAudioInterfaceCompletionHandler)]` struct that signals a `std::sync::mpsc` channel; wait ≤ 5 s, then `GetActivateResult`.
  4. `IAudioClient::Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, 200 ms in 100-ns units, 0, &format, None)` with the fixed format from Global Constraints; `SetEventHandle(CreateEventW(...))`; `GetService::<IAudioCaptureClient>()`; `Start()`.
  5. Loop while running: `WaitForSingleObject(event, 100)`; drain `GetNextPacketSize` / `GetBuffer` / `ReleaseBuffer`; when `AUDCLNT_BUFFERFLAGS_SILENT` is set, pass zeros of the same length; call `on_samples`.
  6. On exit: `Stop()`, close the event, `CoUninitialize()`.

- [ ] **Step 1: Write failing tests** (`#[cfg(all(test, windows))]`):
  - `version_check_is_true_on_this_machine` (dev machine is Windows 11 build 26300 → `true`).
  - `excluding_self_starts_and_stops`: `ProcessLoopback::start(std::process::id(), false, |_| {})` → `Ok`; `stop()`.
  - `stop_joins_capture_thread_quickly`: measure `stop()` < 300 ms.
  - `silence_produces_no_error`: run 500 ms with nothing playing; the callback may or may not be called; no error.
- [ ] **Step 2: Run** `cargo test -p finch-screen process_loopback` → FAIL (module missing).
- [ ] **Step 3: Implement** `process_loopback.rs`.
- [ ] **Step 4: Run** `cargo test -p finch-screen` → PASS; `cargo clippy -p finch-screen --all-targets -- -D warnings` clean.
- [ ] **Step 5: Commit** `feat(screen-audio): capture system sound without FinchSpeak via process loopback`.

### Task 3: Use process loopback by default, fall back on old Windows

**Files:**
- Modify: `crates/finch-screen/src/audio_loopback.rs` (`ScreenAudioCapturer::start`)
- Modify: `app/src/commands/screen.rs:131-153` (emit which source is in use)
- Modify: `app/frontend/src/types.ts` (event `'screen-audio-source': { source: 'all-except-self' | 'only-process' | 'device' }`), `app/frontend/src/stores/screenStore.ts` (store it in `export const [screenAudioSource, setScreenAudioSource] = createSignal<…|null>(null)`)

**Interfaces:**
- Consumes: Task 1 `AudioSourceKind`, Task 2 `ProcessLoopback`, `process_loopback_supported()`.
- Produces: `fn resolve_source(wanted: &AudioSourceKind, supported: bool) -> AudioSourceKind` (pure): when `!supported` → `Device`; otherwise `wanted`. `start` uses it; when process loopback `start` returns an error it logs and falls back to `Device` too. With process loopback `source_rate = 48_000`, `source_channels = 2`, and `on_samples` pushes into the ring-buffer producer (`prod.push_slice`).
- The Tauri command emits `screen-audio-source` after the capturer starts, with the kind actually used (`capturer.source()`).

- [ ] **Step 1: Write failing tests:** `old_windows_falls_back_to_device_loopback` (`resolve_source(&AllExceptSelf, false) == Device`), `new_windows_keeps_the_wanted_source` (`resolve_source(&OnlyProcess(7), true) == OnlyProcess(7)`); on Windows: `capturer_reports_all_except_self` (start with default config, `source() == &AllExceptSelf`).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-screen`, `cargo test -p finch-app`, `npx vitest run` → PASS.
- [ ] **Step 5: Commit** `fix(screen-audio): viewers no longer hear themselves in shared sound`.

### Task 4: Honest copy in the share picker + end-to-end check

**Files:**
- Modify: `app/frontend/src/components/screen/SharePickerModal.tsx:160-173`
- Modify: `app/frontend/src/i18n/index.ts`
- Modify: `docs/audit/manual-voice-checklist.md` (new item)
- Test: `app/frontend/src/components/screen/*.test.tsx` (pick the existing picker test file; create `sharePicker.test.tsx` if none exists)

**Interfaces:**
- Consumes: `screenAudioSource()` from Task 3, and a new `api.processLoopbackSupported(): Promise<boolean>` (Tauri command `process_loopback_supported` in `app/src/commands/screen.rs`, registered in `lib.rs`; returns `false` outside Windows) used before sharing starts.
- Copy (replace both old strings, delete them from `ru`):
  - supported: hint `'Everything your computer plays goes out with the picture, except voices from FinchSpeak.'` → `'Весь звук компьютера уйдёт вместе с картинкой, кроме голосов из FinchSpeak.'`; no amber banner.
  - not supported: hint `'Everything your speakers play goes out with the picture.'` (existing ru), banner `'This version of Windows cannot leave FinchSpeak out of the sound: people in the room will hear themselves with a delay. Update to Windows 10 2004 or newer.'` → `'Эта версия Windows не умеет исключать FinchSpeak из звука: собеседники услышат себя с задержкой. Обновите Windows до 10 2004 или новее.'`

- [ ] **Step 1: Write failing tests:** with `processLoopbackSupported` → `true` and sound toggled on, the amber banner is absent and the new hint is shown; with `false`, the new banner is shown.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Manual check** (needs ears; write it into the checklist, do not claim it): two clients A (sharing with sound) and B (watching); B speaks → B hears no echo from the stream; a YouTube video on A → B hears it; A's FinchSpeak output set to a non-default device → still no echo.
- [ ] **Step 6: Commit** `fix(screen-audio): explain what shared sound includes`.

### Task 5: "Only this window's sound" when sharing a window

**Files:**
- Modify: `app/src/commands/screen.rs` (`start_screen_share`: new arg `audio_scope: Option<String>` = `"system"` | `"window"`), `crates/finch-screen/src/capture.rs` (expose the window's process id: `pub fn window_process_id(window_id: u32) -> Option<u32>` via `xcap::Window::pid()`)
- Modify: `app/frontend/src/components/screen/SharePickerModal.tsx` (segmented control under the sound toggle, only when a window is selected and loopback is supported), `app/frontend/src/stores/screenStore.ts` (`startSharing(kind, id, withAudio, label, audioScope: 'system' | 'window' = 'system')`), `app/frontend/src/services/api.ts`
- Test: Rust unit test for `scope_to_source`; vitest for the control

**Interfaces:**
- Consumes: Task 1–3.
- Produces: `fn scope_to_source(scope: Option<&str>, kind: &str, window_pid: Option<u32>) -> AudioSourceKind` in `screen.rs`: `("window", "window", Some(pid))` → `OnlyProcess(pid)`; everything else → `AllExceptSelf`.
- Copy: `'Sound'` → `'Звук'`; options `'Whole computer'` → `'Весь компьютер'`, `'Only this window'` → `'Только это окно'`.

- [ ] **Step 1: Write failing tests:** `scope_to_source` table (window+pid → `OnlyProcess`; window without pid → `AllExceptSelf`; monitor + "window" → `AllExceptSelf`); vitest: the control appears only for a window source and passes `'window'` to `startSharing`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-app`, `cargo test -p finch-screen`, `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Commit** `feat(screen-audio): share only the selected window's sound`.
