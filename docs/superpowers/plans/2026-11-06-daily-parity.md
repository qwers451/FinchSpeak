# Daily Discord Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The everyday behaviours Discord users expect: "Do not disturb" really silences, global mute/deafen keys, a taskbar badge for mentions, jumping to the channel from a notification, a reconnect that reacts at once after sleep, tray copy in the right language, and no raw English errors in the Russian UI.

**Architecture:** Small, independent changes in the Tauri backend (`app/src`) and SolidJS stores. Global hotkeys get one owner module (`app/src/hotkeys.rs`) so push-to-talk, mute and deafen no longer fight over `unregister_all`. Error localisation goes through one function, `describeError`.

**Tech Stack:** Rust/Tauri 2 (`tauri-plugin-global-shortcut`, `WebviewWindow::set_overlay_icon`), tokio, SolidJS + vitest.

**Spec:** `docs/UX-AUDIT-2026-11.md` — П8, П9, П10, П11, П12, Н2, Н3 (top‑10 rows 7–8).

## Global Constraints

- Same build/test/commit rules as `2026-11-06-sign-in-and-startup.md`.
- Every new UI string: `t()` + `ru` entry (`src/i18n/coverage.test.ts`).
- Hotkey strings use the format the existing capture UI produces (`shortcutFromEvent` in `app/frontend/src/stores/voiceStore.ts:213`, e.g. `"Control+Shift+M"`, `"F8"`).
- Do not change the voice/audio engine (`app/src/audio_engine.rs`) or `finch-audio` in this plan.

## Review Focus

1. The same key chosen for push-to-talk and for mute → refuse the second assignment with a clear message instead of registering both (Task 2: `same_key_cannot_have_two_roles`).
2. The user is in DND **and** gets mentioned while focused on another channel → badge still counts, no sound, no OS notification (Task 1: `dnd_keeps_unread_but_stays_silent`).
3. Mute key pressed while not in a voice room → nothing happens, no error toast (Task 2, vitest `mute hotkey outside a room is ignored`).
4. Notification shown, user opens the app from the tray 3 minutes later → it must **not** jump channels (jump window is 20 s) (Task 4: `late_focus_does_not_jump`).
5. A backend error that is not in the dictionary → the original text is shown unchanged (never an empty toast) (Task 7: `unknown_error_passes_through`).

---

### Task 1: "Do not disturb" silences notifications and sounds

**Files:**
- Modify: `app/frontend/src/stores/desktop.ts:76-104`
- Test: create `app/frontend/src/stores/desktop.test.ts`

**Interfaces:**
- Consumes: `currentUser()?.status` (`'online' | 'idle' | 'donotdisturb' | 'offline'`, kept up to date by `presence-update` in `sessionStore.ts:122-130`).
- Produces: `export function isDoNotDisturb(): boolean`. `reactToMessage` returns `'none'` before notifying/flashing/playing when it is true (the unread counter is updated elsewhere — `noteIncoming` — and stays). `reactToVoiceMovement` keeps join/leave tones (you are in the call) but skips the OS notification.

- [ ] **Step 1: Write failing tests:** `dnd_keeps_unread_but_stays_silent` (status `donotdisturb`, mentioned, unfocused → `reactToMessage` returns `'none'` and the mocked `@tauri-apps/plugin-notification` `sendNotification` is not called); `online user is notified` (same input, status `online` → `'notified'`).
- [ ] **Step 2: Run** `npx vitest run src/stores/desktop.test.ts` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run` → PASS.
- [ ] **Step 5: Commit** `fix(notifications): do not disturb really silences`.

### Task 2: Global mute and deafen keys

**Files:**
- Create: `app/src/hotkeys.rs`
- Modify: `app/src/lib.rs:58-66` (handler delegates to `hotkeys::on_shortcut`), `app/src/state.rs` (field `hotkeys: Arc<hotkeys::Hotkeys>`), `app/src/commands/voice.rs:185-196` (`set_push_to_talk_key` delegates), `lib.rs` invoke list (`commands::voice::set_global_hotkey`)
- Modify: `app/frontend/src/services/api.ts` (`setGlobalHotkey(role: 'mute' | 'deafen', key: string | null)`), `app/frontend/src/stores/voiceStore.ts` (signals + persistence next to `pushToTalkKey`, `restoreHotkeys()` called once from `App.tsx` on mount), `app/frontend/src/stores/sessionStore.ts` (handlers), `app/frontend/src/types.ts` (`'hotkey-toggle-mute': null; 'hotkey-toggle-deafen': null`), `app/frontend/src/components/settings/SettingsModal.tsx:242-250` (two more rows, same capture button pattern), `app/frontend/src/i18n/index.ts`
- Test: `hotkeys.rs` unit tests; `app/frontend/src/stores/voiceExtras.test.ts`

**Interfaces:**
- Produces (Rust):
  ```rust
  #[derive(Clone, Copy, PartialEq, Eq, Debug)]
  pub enum HotkeyRole { PushToTalk, ToggleMute, ToggleDeafen }
  pub struct Hotkeys { /* Mutex<HashMap<HotkeyRole, String>> */ }
  impl Hotkeys {
      /// Returns the key previously bound to `role` (to unregister). Err if another role already has `key`.
      pub fn bind(&self, role: HotkeyRole, key: Option<String>) -> Result<Option<String>, String>;
      pub fn role_of(&self, key: &str) -> Option<HotkeyRole>;
  }
  pub fn apply(app: &AppHandle, role: HotkeyRole, key: Option<String>) -> Result<(), String>; // unregister old, register new
  pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, pressed: bool);
  ```
  `on_shortcut`: `PushToTalk` → `voice_settings.set_key_down(pressed)` (today's behaviour); `ToggleMute`/`ToggleDeafen` on press only → emit `hotkey-toggle-mute` / `hotkey-toggle-deafen`. Compare keys by `Shortcut::to_string()` / parsed `Shortcut` equality, never by raw user text.
- Produces (command): `set_global_hotkey(role: String /* "mute" | "deafen" */, key: Option<String>) -> Result<(), String>`. Error text for a clash: `"This key is already used for another action"`.
- Frontend: `export const [muteKey, setMuteKey] = createSignal<string|null>(null)`, `[deafenKey, setDeafenKey]`, `chooseHotkey(role: 'mute'|'deafen', key: string|null)`; saved in `localStorage['finchspeak.voiceSettings']` as `muteKey` / `deafenKey` next to `key`. `restoreHotkeys()` re-registers mute/deafen at startup (not only on `joinVoice`). Session handlers call `toggleMute()` / `toggleDeafen()` only when `activeVoiceChannel()` is set.
- Copy: `'Mute / unmute'` (exists in tray copy, add to `ru` if missing) → `'Выключить / включить микрофон'`; `'Deafen / undeafen'` → `'Выключить / включить звук'`; hint `'Works in the background, also in games.'` → `'Работает в фоне, в том числе в играх.'`; clash error `'This key is already used for another action'` → `'Эта клавиша уже назначена на другое действие'`.

- [ ] **Step 1: Write failing tests:** Rust `same_key_cannot_have_two_roles`, `rebinding_returns_the_old_key`, `role_of_finds_bound_key`; vitest `mute hotkey toggles mute in a room` and `mute hotkey outside a room is ignored` (call `sessionHandlers['hotkey-toggle-mute'](null)`).
- [ ] **Step 2: Run** `cargo test -p finch-app hotkeys`, `npx vitest run src/stores/voiceExtras.test.ts` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-app`, clippy, `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Commit** `feat(voice): global mute and deafen keys`.

### Task 3: Unmute from the tray or a key also undeafens

**Files:**
- Modify: `app/frontend/src/stores/voiceStore.ts:264-269`
- Test: `app/frontend/src/stores/voiceStore.test.ts`

**Interfaces:**
- Produces: `toggleMute()` while deafened → `api.setVoiceState(false, false)`, `setIsDeafened(false)`, `setIsMuted(false)` (Discord behaviour). Otherwise unchanged.

- [ ] **Step 1: Write failing test** `unmuting while deafened undeafens too`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run` → PASS.
- [ ] **Step 5: Commit** `fix(voice): unmuting while deafened turns the sound back on`.

### Task 4: Mention badge on the taskbar and tray; open the channel after a notification

**Files:**
- Modify: `app/src/commands/window.rs` (new command `set_attention`), `app/src/tray.rs` (tooltip suffix), `app/src/lib.rs` (register)
- Modify: `app/frontend/src/stores/desktop.ts` (`startDesktopSync` effect; remember the last notified channel), `app/frontend/src/services/api.ts` (`setAttention(mentions: number, language)`)
- Test: `app/src/tray.rs` unit test; `app/frontend/src/stores/desktop.test.ts`

**Interfaces:**
- Consumes: `unread()` from `activityStore.ts:10` (`Record<channelId, { count, mention, mentions? }>`), `isChannelMuted`.
- Produces (Rust): `set_attention(mentions: u32, language: Option<String>, app: AppHandle)`: `mentions > 0` → `main` window `set_overlay_icon(Some(red_badge()))`, else `set_overlay_icon(None)`; tray tooltip becomes `"{state tooltip} · {n} упоминаний"` / `"… · {n} mentions"` (use a plural helper with forms упоминание/упоминания/упоминаний). `pub fn red_badge() -> Image<'static>`: 16×16 transparent RGBA with `with_dot` in `#F23F43`.
- Produces (frontend): `export function mentionCount(): number` (sum of `mentions ?? (mention ? 1 : 0)` over channels that are not muted); an effect calls `api.setAttention(mentionCount(), language)`. `notifyDesktop` records `{ channelId, at: Date.now() }`; on `window` `focus`, if less than **20 000 ms** passed, `selectChannel` of that channel and clear the record.
- Note for the implementer: Tauri 2's notification plugin does not report clicks on Windows desktop, hence the focus-within-20 s rule.

- [ ] **Step 1: Write failing tests:** Rust `badge_is_16px_with_opaque_centre` (pixel (8,8) alpha 255, pixel (0,0) alpha 0) and `mention_tooltip_uses_russian_plural` (1 → "упоминание", 3 → "упоминания", 5 → "упоминаний"); vitest `mentionCount ignores muted channels`, `focus_soon_after_notification_jumps`, `late_focus_does_not_jump` (fake timers, 3 min).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-app`, `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Commit** `feat(notifications): mention badge on the taskbar and jump to the channel`.

### Task 5: Reconnect at once after sleep; "Retry now" button

**Files:**
- Modify: `crates/finch-client-core/src/session.rs:355-390` (`reconnect_with_progress` gains a wake signal)
- Modify: `app/src/state.rs` (`reconnect_wake: Arc<tokio::sync::Notify>`), `app/src/connection.rs:175-220` (pass it; new `spawn_sleep_watch`), `app/src/commands/auth.rs` (command `reconnect_now`), `app/src/lib.rs` (register; start `spawn_sleep_watch` in `setup`)
- Modify: `app/frontend/src/components/ConnectionBanner.tsx:20-40`, `app/frontend/src/services/api.ts` (`reconnectNow()`), `app/frontend/src/i18n/index.ts`
- Test: `session.rs` tests; `app/frontend/src/components/*.test.tsx` (create `connectionBanner.test.tsx`)

**Interfaces:**
- Produces: `pub async fn reconnect_with_progress(params, fingerprints, backoff, wake: Arc<tokio::sync::Notify>, on_retry)` — the wait becomes `tokio::select! { _ = sleep(delay) => {}, _ = wake.notified() => { delay = backoff.initial } }`. The convenience `reconnect` wrapper at `session.rs:362` passes a fresh `Notify`.
- Produces: `pub fn spawn_sleep_watch(state: AppState)` — every 5 s compares `SystemTime` elapsed with the expected 5 s; a gap > 30 s means the PC slept → `state.reconnect_wake.notify_one()`. Pure helper `fn slept(expected: Duration, wall: Duration) -> bool` (true iff `wall > expected + 30 s`).
- Produces: command `reconnect_now(state)` → `notify_one()`.
- Banner: while `reconnecting`, a small button `'Retry now'` → `'Повторить сейчас'` calling `api.reconnectNow()`. While `failed` and `!sessionEnded()`, a button `'Back to sign in'` (existing key if present, else add `'Вернуться ко входу'`) calling `logoutUser()`.

- [ ] **Step 1: Write failing tests:** Rust `wake_skips_the_backoff_wait` (backoff initial 10 s, server unreachable, notify after 50 ms → second attempt within 1 s; use the existing reconnect test harness in `session.rs` or `finch-testbed`), `slept_detects_large_gap` (5 s vs 5 s → false; 5 s vs 60 s → true); vitest: banner shows "Retry now" while reconnecting and calls `api.reconnectNow`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-client-core`, `cargo test -p finch-app`, `npx vitest run` → PASS.
- [ ] **Step 5: Commit** `feat(connection): reconnect right after sleep and on demand`.

### Task 6: Tray menu starts in the saved language

**Files:**
- Modify: `app/src/tray.rs:139-140` (`setup` takes a language), `app/src/lib.rs:70-73`, `app/src/commands/window.rs:17-22` (persist on change)
- Test: `tray.rs` unit test

**Interfaces:**
- Produces: `pub fn saved_language(dir: &Path) -> String` / `pub fn save_language(dir: &Path, language: &str)` storing `ui_language.txt` in the profile dir (`profile_directory(...)` from `lib.rs:17`); anything other than `"en"` reads as `"ru"`. `set_tray_state` saves the language when it changes. `tray::setup(app, &saved_language(dir))`.

- [ ] **Step 1: Write failing test** `language_round_trip_defaults_to_ru` (missing file → "ru"; save "en" → "en"; save "xx" → "ru").
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-app` → PASS.
- [ ] **Step 5: Commit** `fix(tray): start in the language the user chose`.

### Task 7: Localise backend errors through one function

**Files:**
- Create: `app/frontend/src/i18n/errors.ts`
- Modify: every `setAppError(String(...))` call site (37 today: `grep -rn "setAppError(String(" app/frontend/src --include=*.ts --include=*.tsx | grep -v test`) and `AuthModal.tsx:19-32` (`describeAuthError` keeps its icons/tones but takes the title from `describeError` when it has no specific rule)
- Modify: `app/frontend/src/i18n/index.ts`, `docs/I18N.md` (one paragraph: "backend errors go through describeError")
- Test: create `app/frontend/src/i18n/errors.test.ts`

**Interfaces:**
- Produces: `export function describeError(error: unknown): string` — strips a leading `Error: `, then tries an ordered list of `{ pattern: RegExp, key: string, params?: (m: RegExpMatchArray) => TranslationParams }` and returns `t(key, params)`; no match → the stripped original text.
  Minimum rules (English key → `ru`):
  | Pattern (case-insensitive) | Key | ru |
  |---|---|---|
  | `^Cannot reach the server: (.*)` | `'Cannot reach the server: {reason}'` | `'Сервер недоступен: {reason}'` |
  | `^Signed in, but the QUIC connection failed` | `'Signed in, but the voice connection failed. Check that UDP port 4433 is open.'` | `'Вход выполнен, но голосовое соединение не установилось. Проверьте, открыт ли UDP-порт 4433.'` |
  | `^Registered, but the QUIC connection failed` | `'Registered, but the voice connection failed. Check that UDP port 4433 is open.'` | `'Регистрация прошла, но голосовое соединение не установилось. Проверьте, открыт ли UDP-порт 4433.'` |
  | `^Registration failed: (.*)` | `'Registration failed: {reason}'` | `'Не удалось зарегистрироваться: {reason}'` |
  | `^Login failed: (.*)` | `'Sign-in failed: {reason}'` | `'Не удалось войти: {reason}'` |
  | `is not trusted yet` | `'Confirm the server certificate, then sign in.'` | `'Подтвердите сертификат сервера, затем войдите.'` |
  | `certificate of .* changed since you trusted it` | `'The server certificate has changed. Do not sign in until the admin confirms the change.'` | `'Сертификат сервера изменился. Не входите, пока администратор не подтвердит замену.'` |
  | `^Logged out locally, but could not revoke` | `'Signed out on this computer. The server could not be reached to end the session.'` | `'Вы вышли на этом компьютере. Сервер недоступен, поэтому сессия на нём не завершена.'` |
  | `^Invalid server address` | `'The server address is not valid'` | `'Неверный адрес сервера'` |

- [ ] **Step 1: Write failing tests:** one case per table row (with `prefs` language `ru`), `strips Error prefix`, `unknown_error_passes_through`.
- [ ] **Step 2: Run** `npx vitest run src/i18n/errors.test.ts` → FAIL.
- [ ] **Step 3: Implement** `describeError` and replace the call sites (`setAppError(String(err))` → `setAppError(describeError(err))`; keep non-error string arguments as they are).
- [ ] **Step 4: Run** `npx vitest run` (including `coverage.test.ts`), `npm run build` → PASS.
- [ ] **Step 5: Commit** `fix(i18n): show backend errors in the user's language`.
