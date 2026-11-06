# Account and First Run Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A "full account": see and end your sessions on other computers, get back in after a forgotten password (admin reset), automatic "Idle" status, stronger passwords; and a first run that needs one click from an invite link instead of dictating address, code and fingerprint.

**Architecture:** Server gets device-aware sessions (`device_name`, `last_seen_at`) and three session routes plus one admin route. The client adds a "Devices" block to the profile page, a system-idle poller, and `finchspeak://join` links handled by `tauri-plugin-deep-link` through the single-instance callback.

**Tech Stack:** Rust (axum, sqlx), Tauri 2 (`tauri-plugin-deep-link`), `windows-sys` 0.61 (`GetLastInputInfo`), SolidJS + vitest.

**Spec:** `docs/UX-AUDIT-2026-11.md` — Н5, Н6, Н7 (auto-idle only), Н9, Л1, Л2, П14 (top‑10 rows 9–10).

**Depends on:** `2026-11-06-sign-in-and-startup.md` (Task 1 `refresh`, Task 3 `finish_sign_in`, Task 5 single instance) must be merged first.

## Global Constraints

- Same build/test/commit rules as `2026-11-06-sign-in-and-startup.md`.
- New migration file: `crates/finch-server/migrations/009_devices.sql` (next number after `008_search.sql`).
- Minimum password length becomes **8** for register, change and admin reset; existing shorter passwords keep working for login.
- Temporary password from admin reset: 12 characters from `ABCDEFGHJKMNPQRSTUVWXYZ23456789` (no look-alikes), shown once, never logged, never stored in clear text.
- Every admin reset writes an audit entry (use the existing helper in `crates/finch-server/src/audit.rs`) with action `"password_reset"`.
- Invite link format (exact): `finchspeak://join?server=<urlencoded https URL>&code=<invite code>&fp=<sha256 fingerprint as shown in the About page>`.

## Review Focus

1. Ending your **own current** session from the device list → treated as "Log out" (local cleanup), not as a dead session (Task 2: `ending_the_current_session_logs_out`).
2. A device that has not refreshed for a while still shows a sensible "last seen" (falls back to `created_at` when `last_seen_at` is NULL for rows from before the migration) (Task 1: `old_sessions_show_created_at_as_last_seen`).
3. Invite link whose `fp` does **not** match the live certificate → the normal TOFU dialog appears with a red note "the link's fingerprint does not match"; nothing is auto-trusted (Task 5: `mismatched_link_fingerprint_is_not_trusted`).
4. Auto-idle never overrides a status the user picked (DND stays DND; manual "Idle" is not reset to "Online" on activity) (Task 4: `auto_idle_respects_manual_status`).
5. Server started from another working directory (Windows shortcut, scheduled task) → it still finds `finch-server.toml` next to the exe and uses the same data folder (Task 6: `config_next_to_exe_wins_when_cwd_has_none`).

---

### Task 1: Device-aware sessions on the server

**Files:**
- Create: `crates/finch-server/migrations/009_devices.sql`:
  ```sql
  ALTER TABLE sessions ADD COLUMN device_name TEXT;
  ALTER TABLE sessions ADD COLUMN last_seen_at INTEGER;
  ```
- Modify: `crates/finch-server/src/api/auth.rs` (login/register accept optional `device_name`; refresh sets `last_seen_at = now`; new handlers), `crates/finch-server/src/api/mod.rs`
- Test: `crates/finch-server/tests/api_test.rs`

**Interfaces:**
- Produces:
  - Login/register request bodies accept `"device_name": string` (optional, trimmed, max 64 chars, stored as given).
  - `GET /api/auth/sessions` → `200 [{ "id": string, "device_name": string|null, "created_at": i64, "last_seen_at": i64, "current": bool }]`, newest `last_seen_at` first; `last_seen_at` = `COALESCE(last_seen_at, created_at)`; only non-expired sessions of the caller.
  - `DELETE /api/auth/sessions/{id}` → `204`; only the caller's own session (else `404`); closes live QUIC connections whose `token_id == id` with reason `b"token revoked"` (same loop as `logout_handler`, `auth.rs:179-187`).
  - `POST /api/auth/sessions/revoke-others` → `204`; deletes all the caller's sessions except the current one and closes their connections.
- [ ] **Step 1: Write failing tests:** `sessions_list_marks_the_current_one` (register + login with `device_name: "LAPTOP"` → list via token 1 has 2 items, the `LAPTOP` one is `current`), `old_sessions_show_created_at_as_last_seen` (set `last_seen_at = NULL` → equals `created_at`), `deleting_another_users_session_is_404`, `revoke_others_keeps_only_current`.
- [ ] **Step 2: Run** `cargo test -p finch-server --test api_test sessions` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-server` → PASS.
- [ ] **Step 5: Commit** `feat(auth): list and end sessions per device`.

### Task 2: "Devices" in the profile settings

**Files:**
- Modify: `crates/finch-client-core/src/api.rs` (`login`/`register` send `device_name`; `list_sessions`, `end_session`, `end_other_sessions`)
- Modify: `app/src/commands/auth.rs` (send `std::env::var("COMPUTERNAME")` as device name; new commands `list_sessions`, `end_session`, `end_other_sessions`), `app/src/lib.rs`
- Modify: `app/frontend/src/services/api.ts`, `app/frontend/src/stores/profileStore.ts`, `app/frontend/src/components/settings/SettingsModal.tsx` (block between "Change password" and "Signed in as", ~line 131), `app/frontend/src/types.ts` (`DeviceSession`), `app/frontend/src/i18n/index.ts`
- Test: `app/frontend/src/components/settings/settings.test.tsx`

**Interfaces:**
- Consumes: Task 1 routes.
- Produces: `type DeviceSession = { id: string; device_name: string | null; created_at: number; last_seen_at: number; current: boolean }`; `loadSessions(): Promise<DeviceSession[]>`, `endSession(id: string): Promise<void>` (if the session is `current` → `logoutUser()` instead), `endOtherSessions(): Promise<void>` in `profileStore.ts`.
- Copy: `'Devices'` → `'Устройства'`; `'This computer'` → `'Этот компьютер'`; `'Unknown device'` → `'Неизвестное устройство'`; `'Last active {when}'` → `'Был активен {when}'` (reuse the existing relative-time helper used by chat timestamps if one exists, else `toLocaleString`); `'End'` → `'Завершить'`; `'Sign out everywhere else'` → `'Выйти на всех остальных устройствах'`.
- [ ] **Step 1: Write failing tests:** the list renders device names and marks "This computer"; `ending_the_current_session_logs_out` (calls `logoutUser`, not `api.endSession`); "Sign out everywhere else" calls `api.endOtherSessions`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-client-core`, `cargo test -p finch-app`, `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Commit** `feat(account): see and end your sessions on other devices`.

### Task 3: Admin password reset and the 8-character minimum

**Files:**
- Modify: `crates/finch-server/src/api/admin.rs` (new `reset_password_handler`), `crates/finch-server/src/api/mod.rs` (`POST /api/admin/users/{id}/password-reset`), `crates/finch-server/src/api/auth.rs:241,504` (min length 8)
- Modify: `crates/finch-client-core/src/api.rs` (`reset_password(user_id) -> Result<String, ClientError>`), `app/src/commands/admin.rs` (`reset_user_password`), `app/src/lib.rs`
- Modify: `app/frontend/src/components/admin/AdminModal.tsx` (button in the user row next to kick/ban; result dialog with the password and a Copy button), `app/frontend/src/stores/adminStore.ts`, `app/frontend/src/ui/passwordStrength.ts` (if it encodes a 6-char minimum, raise to 8), `app/frontend/src/i18n/index.ts`
- Test: `crates/finch-server/tests/api_test.rs`; `app/frontend/src/components/admin/admin.test.tsx`

**Interfaces:**
- Produces: `POST /api/admin/users/{id}/password-reset` (admin only; `403` for members; `404` unknown user; an admin cannot reset another admin → `403`) → `200 { "temporary_password": string }`. It sets the new Argon2 hash (`hash_password`), deletes **all** sessions of that user, closes their QUIC connections with reason `b"password changed"`, and writes the audit entry.
- Copy: `'Reset password'` → `'Сбросить пароль'`; dialog title `'New temporary password'` → `'Новый временный пароль'`; body `'Give it to {name} privately. They should change it in Settings → Profile after signing in.'` → `'Передайте его {name} лично. После входа пусть сменит его в «Настройки → Профиль».'`; register/change error `'The password must be at least 8 characters'` → `'Пароль должен быть не короче 8 символов'` (server message text must match the key so `describeError` or a direct `t()` can translate it).
- [ ] **Step 1: Write failing tests:** `admin_reset_issues_a_working_temporary_password` (login with it → 200, old password → 401, old token → 401), `member_cannot_reset`, `admin_cannot_reset_another_admin`, `reset_is_audited`, `short_password_is_rejected_at_registration` (7 chars → 400); vitest: the dialog shows the password once and Copy writes it to the clipboard.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-server`, `cargo test -p finch-app`, `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Commit** `feat(admin): reset a member's password; require 8 characters`.

### Task 4: Automatic "Idle" after 10 minutes away

**Files:**
- Modify: `app/Cargo.toml` (`[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.61", features = ["Win32_UI_Input_KeyboardAndMouse", "Win32_System_SystemInformation"] }`), `app/src/commands/window.rs` (command `system_idle_seconds`), `app/src/lib.rs`
- Create: `app/frontend/src/stores/idleStore.ts`
- Modify: `app/frontend/src/App.tsx` (start after sign-in), `app/frontend/src/services/api.ts` (`systemIdleSeconds(): Promise<number>`; `0` outside Tauri)
- Test: create `app/frontend/src/stores/idleStore.test.ts`

**Interfaces:**
- Produces (Rust): `system_idle_seconds() -> u64` via `GetLastInputInfo` and `GetTickCount` (wrapping subtraction), `0` on failure or non-Windows.
- Produces (frontend): `startAutoIdle(): () => void` — polls every 30 s. `AUTO_IDLE_AFTER_S = 600`. When idle ≥ 600 s and the user's own status is `'online'`, call `saveStatus('idle', currentText)` from `profileStore.ts` and remember `autoIdled = true`. When idle < 600 s and `autoIdled`, restore `'online'` and clear the flag. Never touches `'donotdisturb'` or a manually chosen `'idle'`.
- [ ] **Step 1: Write failing tests** (fake timers, mocked `api.systemIdleSeconds`, spied `saveStatus`): goes idle at 600 s; returns online on activity; `auto_idle_respects_manual_status` (DND stays; manual idle is not reset).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-app`, `npx vitest run` → PASS.
- [ ] **Step 5: Commit** `feat(presence): go idle automatically after 10 minutes away`.

### Task 5: Invite links `finchspeak://join` and an empty default server

**Files:**
- Modify: `app/Cargo.toml` (`tauri-plugin-deep-link = "2"`), `app/tauri.conf.json` (`"plugins": { "deep-link": { "desktop": { "schemes": ["finchspeak"] } } }`), `app/capabilities/default.json` (`"deep-link:default"`), `app/src/lib.rs` (register the plugin; in the single-instance callback and at startup forward any `finchspeak://` argument as event `invite-link` with the raw URL; on Windows call `app.deep_link().register_all()` in `setup`)
- Create: `app/frontend/src/stores/inviteLink.ts`
- Modify: `app/frontend/src/stores/authStore.ts:13` (`DEFAULT_SERVER = ''`), `app/frontend/src/components/AuthModal.tsx` (prefill from a pending link, switch to register mode), `app/frontend/src/components/TofuModal.tsx` (auto-trust when the link's fingerprint matches), `app/frontend/src/components/admin/AdminModal.tsx:151` (button "Copy invite link" next to "Copy"), `app/frontend/src/types.ts` (`'invite-link': string`), `app/frontend/src/stores/sessionStore.ts` (handler), `app/frontend/src/i18n/index.ts`
- Test: create `app/frontend/src/stores/inviteLink.test.ts`; `app/frontend/src/components/authScreens.test.tsx`

**Interfaces:**
- Produces: `parseInviteLink(url: string): { server: string; code: string; fp: string | null } | null` (only scheme `finchspeak:`, host/path `join`, `server` must be `https://`; otherwise `null`); `buildInviteLink(server: string, code: string, fp: string): string` (exact format from Global Constraints); `export const [pendingInvite, setPendingInvite] = createSignal<ReturnType<typeof parseInviteLink>>(null)`.
- Fingerprint comparison ignores case, spaces and colons. Match → `TofuModal` calls the same confirm path without showing the dialog; mismatch → dialog shows with the red note `'The fingerprint in the invite link does not match this server.'` → `'Отпечаток в ссылке-приглашении не совпадает с сервером.'`.
- Copy: `'Copy invite link'` → `'Скопировать ссылку-приглашение'`; server field placeholder `'https://your-server:8443'` (no translation needed) and empty-field error `'Enter the server address'` → `'Укажите адрес сервера'`.
- [ ] **Step 1: Write failing tests:** parse/build round trip; rejects `http://` and other schemes; `mismatched_link_fingerprint_is_not_trusted`; a matching link prefills server and invite code and opens register mode; submitting with an empty server shows `'Enter the server address'`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-app`, `npx vitest run`, `npm run build` → PASS.
- [ ] **Step 5: Manual check:** build the installer, install, open `finchspeak://join?...` from Win+R → FinchSpeak opens (or focuses) on the register form with the fields filled; no certificate dialog when the fingerprint matches.
- [ ] **Step 6: Commit** `feat(onboarding): one-click invite links with a pinned fingerprint`.

### Task 6: Server finds its config next to the exe

**Files:**
- Modify: `crates/finch-server/src/main.rs:48-51`
- Test: unit test in `main.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `fn working_dir_for_config(cwd: &Path, exe_dir: &Path, name: &str) -> Option<PathBuf>` — `None` when `cwd/name` exists; `Some(exe_dir)` when it does not but `exe_dir/name` does; `None` otherwise. `main` calls `std::env::set_current_dir` with it (and logs `info!("Using the configuration next to the executable: {dir:?}")`) **before** loading the config, so every relative `./data/...` path resolves to the same place as before.
- [ ] **Step 1: Write failing test** `config_next_to_exe_wins_when_cwd_has_none` (temp dirs: config only in exe dir → `Some(exe_dir)`; config in cwd → `None`; nowhere → `None`).
- [ ] **Step 2: Run** `cargo test -p finch-server --bin finch-server working_dir` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p finch-server` → PASS.
- [ ] **Step 5: Commit** `fix(server): use the config next to the exe when started elsewhere`.
