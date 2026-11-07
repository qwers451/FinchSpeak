# Sign-in and Startup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** FinchSpeak keeps the user signed in across restarts, updates and sleep, runs as a single instance, can start with Windows minimised to the tray, and finishes the first sign-in without a second click.

**Architecture:** The server turns the fixed 72 h session into a sliding one (`POST /api/auth/refresh` extends the same session row and re-signs the JWT with the same `jti`). The Tauri backend keeps the JWT in Windows Credential Manager behind a small `TokenStore` trait, resumes the session at startup and refreshes it every 6 h. Single-instance and autostart come from the official Tauri 2 plugins.

**Tech Stack:** Rust (axum, sqlx/SQLite, jsonwebtoken), Tauri 2 (`tauri-plugin-single-instance`, `tauri-plugin-autostart`), `keyring` 3 (`windows-native`), SolidJS + vitest.

**Spec:** `docs/UX-AUDIT-2026-11.md` — items П1, П2, П3, П4, П5, П6, П7, П13 (top‑10 rows 1–6 and 10, first half).

## Global Constraints

- Build env: `FINCH_WEBRTC_ROOT=<repo>/target/native/Release` must be set for any `cargo` command that builds `finch-app`.
- Before every commit: `cargo test -p <crate you touched>`, `cargo clippy -p <crate> --all-targets -- -D warnings`; frontend: `npx vitest run` and `npm run build` in `app/frontend`.
- No `unwrap()/expect()/todo!()` in server, network or client code outside `#[cfg(test)]`.
- The password is never stored anywhere. Only the JWT goes to Credential Manager.
- Every new UI string goes through `t()` and gets a Russian entry in `app/frontend/src/i18n/index.ts` (`ru` map); `src/i18n/coverage.test.ts` fails otherwise. See `docs/I18N.md`.
- `finch-server` must not depend on `finch-audio`, `finch-screen` or `finch-client-core`.
- One task = one commit. Messages: `feat(auth): …`, `fix(auth): …`, `feat(app): …`. End each commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Session window: `auth.jwt_expiration_hours` (alias `token_expiry_hours`) becomes the **sliding** window. New default: `720` (30 days). `deploy/finch-server.toml` and `deploy/finch-server.windows.toml` set `token_expiry_hours = 720`.
- Credential Manager entry: service `"FinchSpeak"`, account `"{server_url}|{username}"` (server URL with trailing `/` trimmed).

## Review Focus

1. Credential Manager unavailable or the entry is corrupt → the app shows the normal sign-in form, never an error screen (test in Task 3: `resume_returns_none_when_store_is_empty`).
2. The stored token belongs to a session that was revoked (password changed elsewhere, logout on another device) → resume deletes the stored token and shows the sign-in form (Task 3: `resume_deletes_token_rejected_by_server`).
3. The server is unreachable at startup → resume keeps the stored token and the form shows "Cannot reach the server" with Retry; the next start tries again (Task 4: `resume_failure_keeps_user_on_form_with_error`).
4. Second launch while the first is hidden in the tray → no new process window; the existing window is shown and focused (Task 5, manual check step).
5. Autostart launches with `--minimized` before the user is signed in and resume fails → the app stays in the tray with no window, and the tray icon "Open" shows the form (Task 6, manual check step).

---

### Task 1: Sliding server sessions and `POST /api/auth/refresh`

**Files:**
- Modify: `crates/finch-server/src/api/auth.rs` (`issue_jwt` at 119-150, new `refresh_handler`)
- Modify: `crates/finch-server/src/api/mod.rs:39-45` (route)
- Modify: `crates/finch-server/src/config.rs:253-255` (default 720), the test at `config.rs:330`
- Modify: `deploy/finch-server.toml:21`, `deploy/finch-server.windows.toml:16`
- Test: `crates/finch-server/tests/api_test.rs`

**Interfaces:**
- Produces: `pub fn issue_jwt_for_session(user_id: &Uuid, username: &str, role: &str, secret: &str, expiration_hours: i64, jti: &str) -> Result<String, ServerError>`; `issue_jwt` keeps its signature and calls it with a fresh `jti`.
- Produces: `POST /api/auth/refresh` (Bearer auth) → `200` with the same `AuthResponse` JSON as login (`token`, `user`, `quic_port`). It sets `sessions.expires_at = now + jwt_expiration_hours*3600` for `auth.session_id` and returns a JWT with the same `jti` and the new `exp`. Expired/revoked token → `401`.

- [ ] **Step 1: Write the failing tests** in `api_test.rs`, using the `aud039_*` pattern (register via `oneshot`, read `token`):
  - `refresh_extends_the_same_session`: register alice; `UPDATE sessions SET expires_at = expires_at - 3600`; read `expires_at` as `before`; `POST /api/auth/refresh` with the token → `200`; new token `!=` old token; `SELECT COUNT(*) FROM sessions` is `1`; `expires_at > before`; `GET /api/auth/me` with the new token → `200`.
  - `refresh_rejects_a_revoked_session`: register, `DELETE FROM sessions`, refresh → `401`.
  - `refreshed_token_survives_logout_of_other_device`: register + login (two tokens), logout token 0, refresh token 1 → `200`.
- [ ] **Step 2: Run** `cargo test -p finch-server --test api_test refresh` → FAIL (404 on the route).
- [ ] **Step 3: Implement** `issue_jwt_for_session`, `refresh_handler(auth: AuthUser, State(state): State<ArcSharedState>) -> Result<Json<AuthResponse>, ServerError>` (load `display_name`, `avatar_version`, `role` from `users` the same way `login_handler` does at 380-425; `UPDATE sessions SET expires_at = ? WHERE id = ? AND user_id = ?`), and the route. Change the default to `720` and the deploy files. Update the config test that asserts `72` to `720`.
- [ ] **Step 4: Run** `cargo test -p finch-server` → PASS; clippy clean.
- [ ] **Step 5: Commit** `feat(auth): sliding sessions with POST /api/auth/refresh`.

### Task 2: Client refresh call and device-safe token store

**Files:**
- Modify: `crates/finch-client-core/src/api.rs` (next to `me()` at 435)
- Create: `app/src/credentials.rs`
- Modify: `app/Cargo.toml` (`keyring = { version = "3", features = ["windows-native"] }`), `app/src/lib.rs` (`mod credentials;`), `app/src/state.rs` (field `token_store`)

**Interfaces:**
- Produces in `finch-client-core`: `pub async fn refresh(&self) -> Result<AuthResponse, ClientError>` (`POST /api/auth/refresh`, authorized; 401 must map to the same `ClientError` variant that `me()` returns for 401 — reuse `parse_json`).
- Produces in `app/src/credentials.rs`:
  ```rust
  pub trait TokenStore: Send + Sync {
      fn load(&self, key: &str) -> Option<String>;
      fn save(&self, key: &str, token: &str) -> Result<(), String>;
      fn delete(&self, key: &str);
  }
  pub struct KeyringStore;            // service "FinchSpeak"
  pub struct MemoryStore(std::sync::Mutex<std::collections::HashMap<String, String>>); // tests
  pub fn credential_key(server_url: &str, username: &str) -> String; // "{url without trailing /}|{username}"
  ```
  `KeyringStore::load` returns `None` on any keyring error (Review Focus 1).
- Produces in `AppState`: `pub token_store: Arc<dyn TokenStore>` (default `KeyringStore`), `pub signed_in_key: Arc<RwLock<Option<String>>>` (the key of the current session, used by logout and refresh).

- [ ] **Step 1: Write failing unit tests** in `credentials.rs` `#[cfg(test)]`: `credential_key_trims_trailing_slash` (`credential_key("https://a:8443/", "bob") == "https://a:8443|bob"`), `memory_store_round_trip` (save → load `Some`, delete → load `None`).
- [ ] **Step 2: Run** `cargo test -p finch-app credentials` → FAIL (module missing).
- [ ] **Step 3: Implement** the module, the `refresh()` client call, and the `AppState` fields (initialise in `AppState::new` at `state.rs:247-270`).
- [ ] **Step 4: Run** `cargo test -p finch-app credentials` and `cargo test -p finch-client-core` → PASS.
- [ ] **Step 5: Commit** `feat(app): token store backed by Windows Credential Manager`.

### Task 3: `resume_session`, remembered sign-in and periodic refresh (backend)

**Files:**
- Modify: `app/src/commands/auth.rs` (login 90-137, register 139-184, logout 186-219; new `resume_session`)
- Modify: `app/src/connection.rs` (new `spawn_token_refresher`)
- Modify: `app/src/lib.rs:100-115` (register `commands::auth::resume_session`)
- Test: `app/src/commands/auth.rs` `#[cfg(test)]` using `finch-testbed` (see `app/src/connection.rs:561` for the `TestServer`/`TestClient` pattern)

**Interfaces:**
- Consumes: Task 2 (`TokenStore`, `credential_key`, `ApiClient::refresh`).
- Produces (commands):
  - `login(server_url, username, password, remember: Option<bool>)` — `remember` defaults to `true`; on success with `remember` it saves the JWT under `credential_key(server_url, username)` and sets `signed_in_key`.
  - `register(..., remember: Option<bool>)` — same.
  - `resume_session(server_url: String, username: String) -> Result<Option<UserDto>, String>`:
    no stored token → `Ok(None)`; `trusted_api` fails with untrusted/mismatch → `Err(message)` (events are emitted as today); `refresh()` returns 401 → delete stored token, `Ok(None)`; network error → `Err("Cannot reach the server: …")` and the token is kept; success → save the new token, run the same steps as login after `api.login` (state fields + `start_quic_session`), `Ok(Some(user))`.
  - `logout` additionally deletes the stored token for `signed_in_key` and clears it.
- Produces (private): `async fn finish_sign_in(api: ApiClient, auth: AuthResponse, server_url: &str, state: &AppState, app_handle: AppHandle) -> Result<UserDto, String>` — the shared tail of login/register/resume (lines 105-136 today).
- Produces: `pub fn spawn_token_refresher(state: AppState, generation: u64)` — every 6 h while `session_generation == generation`: `api.refresh()`, then update `api` token, `auth_token`, `last_connect.token` and the stored token. Errors are logged and retried next tick. Called from `finish_sign_in`.

- [ ] **Step 1: Write failing tests** (testbed server, `AppState` with `MemoryStore`):
  - `resume_returns_none_when_store_is_empty`.
  - `resume_restores_session_from_stored_token`: login with `remember = true`, simulate a restart by building a fresh `AppState` sharing the same `MemoryStore` and fingerprints, `resume_session` → `Some(user)` with the same `id`.
  - `resume_deletes_token_rejected_by_server`: store a token, `DELETE FROM sessions` on the testbed DB, resume → `None` and the store is empty.
  - `logout_forgets_the_stored_token`.
  If the testbed cannot provide an `AppHandle`, test the private helpers that take `&AppState` and leave the command wrapper thin.
- [ ] **Step 2: Run** `cargo test -p finch-app resume` → FAIL.
- [ ] **Step 3: Implement** `finish_sign_in`, the `remember` parameter, `resume_session`, logout cleanup and `spawn_token_refresher`.
- [ ] **Step 4: Run** `cargo test -p finch-app` → PASS; clippy clean.
- [ ] **Step 5: Commit** `feat(auth): stay signed in across restarts`.

### Task 4: Frontend auto sign-in and "Stay signed in"

**Files:**
- Modify: `app/frontend/src/services/api.ts:13-60` (`login`/`register` gain `remember`; new `resumeSession`)
- Modify: `app/frontend/src/stores/authStore.ts` (new `resumeLastSession`; `loginUser`/`registerUser` take `remember`)
- Modify: `app/frontend/src/App.tsx:29-48` (call resume when `getCurrentUser()` is `null`)
- Modify: `app/frontend/src/components/AuthModal.tsx` (checkbox above the submit button at ~120)
- Modify: `app/frontend/src/i18n/index.ts`
- Test: `app/frontend/src/stores/authStore.test.ts`, `app/frontend/src/components/authScreens.test.tsx`

**Interfaces:**
- Consumes: Task 3 commands (`resume_session` with `{ serverUrl, username }`; `login`/`register` with `remember`).
- Produces: `api.resumeSession(serverUrl: string, username: string): Promise<User | null>`; `resumeLastSession(): Promise<boolean>` in `authStore.ts` (no-op `false` when `rememberedUsername()` is empty; on success `setCurrentUser` + `loadChannels`; on error `setAuthError(String(err))` and `false`). `export const [resuming, setResuming] = createSignal(false)` — `App.tsx` keeps `<Splash/>` while `resuming()`.
- Copy: checkbox label `'Stay signed in'` → `'Не выходить из аккаунта'`, checked by default.

- [ ] **Step 1: Write failing tests:**
  - `authStore.test.ts`: `resumeLastSession signs in with the remembered account` (mock `api.resumeSession` → user; expect `currentUser()` set); `resume_failure_keeps_user_on_form_with_error` (mock reject `'Cannot reach the server: timeout'`; expect `currentUser()` `null` and `authError()` contains `'Cannot reach'`); `resume is skipped without a remembered username`.
  - `authScreens.test.tsx`: `stay signed in is on by default and is passed to login` (render `AuthModal`, submit, expect `api.login` called with `remember === true`).
- [ ] **Step 2: Run** `npx vitest run src/stores/authStore.test.ts src/components/authScreens.test.tsx` → FAIL.
- [ ] **Step 3: Implement** the API wrapper, store function, `App.tsx` call and checkbox (use `ui/Toggle` or a native checkbox styled like `Field`; follow existing markup). Add the `ru` entries.
- [ ] **Step 4: Run** `npx vitest run` and `npm run build` → PASS.
- [ ] **Step 5: Commit** `feat(auth): sign in automatically to the last server`.

### Task 5: Single instance

**Files:**
- Modify: `app/Cargo.toml` (`tauri-plugin-single-instance = "2"`)
- Modify: `app/src/lib.rs:55` (register the plugin **first**, before `tauri_plugin_opener`)

**Interfaces:**
- Produces: a second launch calls `tray::show_window(app)` on the running instance (the same function `show_main_window` uses, `app/src/commands/window.rs:24-28`) and exits. The callback receives `argv`; keep it — the onboarding plan uses it for `finchspeak://` links.

- [ ] **Step 1: Add the plugin**: `.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| tray::show_window(app)))`.
- [ ] **Step 2: Run** `cargo clippy -p finch-app --all-targets -- -D warnings` and `cargo test -p finch-app` → PASS.
- [ ] **Step 3: Manual check** (controller or user): `npm run tauri build` or a dev build, start the app, close to tray, start it again from the exe → one tray icon, the window comes back, still signed in. Record the result in the commit body.
- [ ] **Step 4: Commit** `feat(app): run a single instance and focus it on relaunch`.

### Task 6: Autostart with Windows, minimised to the tray

**Files:**
- Modify: `app/Cargo.toml` (`tauri-plugin-autostart = "2"`), `app/frontend/package.json` (`@tauri-apps/plugin-autostart` `^2`)
- Modify: `app/src/lib.rs:55-89` (plugin; `setup` skips `show()/set_focus()` when `std::env::args().any(|a| a == "--minimized")`)
- Modify: `app/tauri.conf.json` (`"visible": false` for `main`; `setup` shows it explicitly when not minimised)
- Modify: `app/capabilities/default.json` (`"autostart:allow-enable"`, `"autostart:allow-disable"`, `"autostart:allow-is-enabled"`)
- Modify: `app/frontend/src/services/api.ts` (`autostartEnabled(): Promise<boolean>`, `setAutostart(on: boolean): Promise<void>`; both no-ops returning `false` outside Tauri)
- Modify: `app/frontend/src/components/settings/SettingsModal.tsx:189` (new row right after "Close button hides to the tray")
- Test: `app/frontend/src/components/settings/settings.test.tsx`

**Interfaces:**
- Produces: `pub fn should_start_hidden(args: impl IntoIterator<Item = String>) -> bool` in `lib.rs` (true iff an argument equals `"--minimized"`), used by `setup`.
- Copy: `'Start with Windows'` → `'Запускать вместе с Windows'`; hint `'Starts hidden in the tray and signs you in.'` → `'Запускается свёрнутым в трей и входит в аккаунт.'`.
- The toggle reads its state from `api.autostartEnabled()` on mount (truth lives in the OS, not localStorage).

- [ ] **Step 1: Write failing tests**: Rust `should_start_hidden_only_with_flag` (`["finch-app.exe"]` → false; `["finch-app.exe","--minimized"]` → true); vitest `autostart toggle reflects and changes the OS setting` (mock `api.autostartEnabled` → `false`, click toggle, expect `api.setAutostart(true)`).
- [ ] **Step 2: Run** both → FAIL.
- [ ] **Step 3: Implement** (plugin init: `tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"]))`).
- [ ] **Step 4: Run** `cargo test -p finch-app`, `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Manual check:** enable the toggle, check `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` has `FinchSpeak … --minimized`, sign out of Windows and back in → tray icon present, no window, signed in.
- [ ] **Step 6: Commit** `feat(app): start with Windows minimised to the tray`.

### Task 7: Honest session-ended reasons

**Files:**
- Modify: `crates/finch-server/src/api/auth.rs:153-177` (`verify_jwt` / `verify_active_jwt`), `crates/finch-server/src/quic.rs:210-219`
- Modify: `app/frontend/src/stores/sessionEnd.ts`
- Modify: `app/frontend/src/i18n/index.ts`
- Test: `crates/finch-server/tests/api_test.rs` or a unit test in `auth.rs`; `app/frontend/src/components/overlays/overlays.test.tsx`

**Interfaces:**
- Produces (server): `pub enum TokenProblem { Expired, Revoked, Invalid }` and `pub async fn check_active_jwt(token, secret, pool) -> Result<Claims, TokenProblem>`; `verify_active_jwt` keeps its signature (maps any problem to `ServerError::Unauthorized`). QUIC rejection message becomes exactly `"Session expired"`, `"Session revoked"` or `"Invalid token"`.
  `Expired` = `jsonwebtoken` `ErrorKind::ExpiredSignature` **or** the session row exists with `expires_at <= now`; `Revoked` = signature valid but no session row.
- Produces (client): `SessionEndKind = 'kicked' | 'banned' | 'signed-out' | 'expired' | 'password-changed'`. `classifyEnd`: text contains `'session expired'` → `'expired'`; `'password changed'` → `'password-changed'`; existing rules stay.
- Copy: expired — headline `'Your session has expired'` / `'Срок входа истёк'`, body `'You have not used FinchSpeak on this computer for a long time. Sign in again.'` / `'Вы давно не заходили в FinchSpeak на этом компьютере. Войдите снова.'`; password-changed — headline `'Your password was changed'` / `'Пароль был изменён'`, body `'Sign in with the new password.'` / `'Войдите с новым паролем.'`. The `signed-out` body becomes `'Your session was ended on another device or by an admin. Sign in again to continue.'` (+ ru).

- [ ] **Step 1: Write failing tests:** server `expired_and_revoked_tokens_are_told_apart` (issue a JWT with `expiration_hours = -1` → `Expired`; delete the session row → `Revoked`); vitest `classifyEnd` cases for `'Authentication rejected (401): Session expired'` → `'expired'`, `'password changed'` → `'password-changed'`, `'token revoked'` → `'signed-out'`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-server`, `npx vitest run` → PASS.
- [ ] **Step 5: Commit** `fix(auth): tell expired, revoked and password-changed sessions apart`.

### Task 8: Sign in right after trusting the certificate

**Files:**
- Modify: `app/frontend/src/stores/authStore.ts` (remember the last attempt), `app/frontend/src/components/TofuModal.tsx:25-31`
- Test: `app/frontend/src/components/authScreens.test.tsx`

**Interfaces:**
- Produces: `retryPendingSignIn(): Promise<boolean>` in `authStore.ts`. `loginUser`/`registerUser` store their arguments in a module-level `lastAttempt` (cleared on success, on `logoutUser`, and after one retry). `TofuModal.confirm` calls it after `api.trustFingerprint`. The password lives only in memory and only until the retry.

- [ ] **Step 1: Write failing test** `trusting the server retries the sign-in once`: first `api.login` rejects with `'Server certificate of h is not trusted yet'` and sets `pendingTofu`; confirm in `TofuModal` → `api.login` called a second time with the same username, password and `remember`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run` → PASS.
- [ ] **Step 5: Commit** `fix(auth): finish the first sign-in after the certificate is trusted`.

### Task 9: Check for updates automatically

**Files:**
- Create: `app/frontend/src/stores/updateStore.ts`
- Modify: `app/frontend/src/App.tsx` (start the checker after sign-in), `app/frontend/src/components/settings/SettingsModal.tsx:206-222` (reuse the store), `app/frontend/src/i18n/index.ts`
- Test: `app/frontend/src/stores/updateStore.test.ts`

**Interfaces:**
- Produces: `export const [availableUpdate, setAvailableUpdate] = createSignal<{ version: string; notes: string } | null>(null)`; `startUpdateChecks(now = Date.now): () => void` — first check 15 s after start, then every 24 h; errors are silent (logged with `console.warn`). When an update appears it calls `pushToast({ kind: 'info', title: t('FinchSpeak {version} is available'), text: t('Install it from Settings → Keys and about.') })` once per version.
- Installing stays manual (Settings button) and is disabled while `activeVoiceChannel()` is set, with hint `'Leave the voice room to install.'`. Thanks to Task 3 the user stays signed in after the installer restarts the app (П6).

- [ ] **Step 1: Write failing tests** with `vi.useFakeTimers()`: no check before 15 s; one check at 15 s; second at 15 s + 24 h; a toast once per version.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Commit** `feat(app): check for updates in the background`.

---

## Finish

- [ ] Full gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `npx vitest run`, `npm run build`.
- [ ] Manual scenario list for the user (append to `docs/audit/manual-voice-checklist.md` under a new heading "Вход и запуск"): restart client → signed in; reboot → signed in, in tray; relaunch from Start while in tray → same window; server with `token_expiry_hours = 1`, leave the app closed 2 h → plain sign-in form (resume found an expired token and deleted it); with the default 720 h, leave the app open over a day → still signed in (refresher); change password on device B → device A shows "Пароль был изменён".
