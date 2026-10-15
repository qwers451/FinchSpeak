# Аудит, тестовый стенд и исправления — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Получить отчёт о реальном состоянии FinchSpeak, автоматический стенд (функциональные сценарии и голосовые метрики) и исправить согласованные критичные и важные находки.

**Architecture:** Фаза 1 — отчёт по коду как он есть. Фаза 2:
- голосовой захват выносится из `app/src/audio_engine.rs` в `finch_audio::VoiceCapture`;
- QUIC/REST-клиент выносится из Tauri в новый крейт `finch-client-core`;
- сервер получает функцию запуска в процессе (`finch_server::launch`);
- новый dev-крейт `finch-testbed` поднимает настоящий сервер и headless-клиентов.

Фаза 3 — исправления, каждое через падающий тест.

**Tech Stack:** Rust 2024, tokio, quinn 0.11, rustls 0.23, axum, sqlx/SQLite, reqwest 0.12, Opus, nnnoiseless, Tauri v2, SolidJS. Новые dev-зависимости в стенде: `hound` (WAV), `tempfile`, `rand` (`StdRng`, seed 42).

**Spec:** [docs/superpowers/specs/2026-10-15-audit-and-testbed-design.md](../specs/2026-10-15-audit-and-testbed-design.md)

## Global Constraints

- Ветка `audit-and-testbed`, не `main`.
- Нет `todo!()`, `unwrap()`, `expect()` в сетевом, серверном и аудио-коде (production-код всех крейтов, кроме `#[cfg(test)]` и `finch-testbed/tests`). Ошибки — через `Result` + `thiserror`.
- `MediaHeader` ровно 20 байт; сервер перезаписывает байты 5..9 (`sender_peer_id`).
- `finch-server` не зависит от `finch-audio`, `finch-screen`, `finch-client-core`. От них зависит только `finch-testbed` (`publish = false`).
- Аудио-колбэки `cpal`: без мьютексов и аллокаций, только `ringbuf`.
- Перед каждым коммитом зелёные все проверки: `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `npm run build` (в `app/frontend`).
- Рефакторинг фазы 2 не меняет поведения приложения. Известные баги переносятся как есть и чинятся только в фазе 3 по согласованному отчёту.
- Пороги из спека: задержка ≤ 120 мс; улучшение SNR ≥ 10 дБ при входном SNR 5 дБ; сегментный SNR речевых участков не ниже входного; ERLE ≥ 20 дБ через 2 с; плохая сеть (5% потерь, джиттер ±30 мс, перестановка) — задержка ≤ 200 мс. Порог не понижают без согласования с пользователем.
- Фикстуры речи ≤ 15 с суммарно, все фикстуры < 1 МБ, с файлом атрибуции CC BY 4.0.

## Отклонения от спека (принятые при планировании)

1. **Вместо единого `VoicePipeline` — `VoiceCapture` + существующий `AudioMixer`.** Микшер общий для tokio-задачи приёма датаграмм и аудио-потока (`Arc<Mutex<AudioMixer>>` в `AppState`), поэтому «receive/render» остаются в `AudioMixer`, а в отдельную структуру выносится только захват (AEC → NS → VAD → Opus → пакет).
2. **Без поля ключа E2EE.** Поле, которое ничего не делает, — ловушка. Ключ добавляется вместе с исправлением AUD-001.
3. **Шумоподавление и эхоподавление меряются на выходе `VoiceCapture.process()` до кодирования.** Opus сдвигает фазу, и посэмпловый SNR после декодера бессмыслен. Задержка, VAD и плохая сеть меряются по полной цепочке A → сервер → B.
4. **Сценарий E2EE в фазе 2 проверяет протокольный уровень, без `#[ignore]`.** Зашифрованный пакет проходит через SFU и расшифровывается у получателя, а неверный ключ не расшифровывает. Клиентское шифрование (AUD-001) тестируется в фазе 3.
5. **Ограничение `RequestKeyframe` 1/с на сервере не проверяется**: это клиентская ответственность. Стенд проверяет пересылку запроса стримеру.

## Review Focus

1. **Аудио на разной частоте устройства** (44.1 кГц, стерео) — после рефакторинга захват и воспроизведение работают как раньше. Покрыто тестом `voice_capture_matches_legacy_packet_layout` (Task 3) и ручной проверкой в Task 5.
2. **Сервер не принимает подключения после одного сбойного рукопожатия** (`incoming.await?` в `quic::run_server`). Сценарий `server_survives_failed_handshake` (Task 8) — ожидаемо красный до исправления, помечен `#[ignore = "AUD-xxx"]` по номеру из отчёта.
3. **Клиент отключается посреди звонка** — presence чистится, сервер не дедлочится. Сценарий `abrupt_disconnect_cleans_presence` (Task 8).
4. **Переполнение очереди событий клиента при потоке датаграмм** — датаграммы отбрасываются, управляющие сообщения не теряются. Тест `control_events_survive_datagram_flood` (Task 4).
5. **Перезапуск сервера на той же БД** — сообщения и пользователи сохраняются, клиент переподключается. Сценарий `client_reconnects_after_server_restart` (Task 8).

---

## Фаза 1 — отчёт

### Task 1: Отчёт аудита

**Files:**
- Create: `docs/audit/2026-10-audit-report.md`

**Interfaces:**
- Produces: список находок `AUD-001…AUD-NNN` в формате спека (раздел 5). Фаза 3 и пометки `#[ignore = "AUD-NNN"]` ссылаются на эти ID.

- [ ] **Step 1: Ревью кода по модулям** в порядке `finch-protocol` → `finch-server` → `finch-audio` → `finch-screen` → `app/src` → `app/frontend/src`. В каждом модуле проверить: корректность, ошибки и их обработку, гонки и блокировки (`DashMap`, `Mutex` через `.await`), утечки задач, безопасность входных данных, соответствие требованиям 1–11 PROGRESS.md.

- [ ] **Step 2: Внести в отчёт находки, уже найденные при подготовке спека и плана**, проверив каждую по коду:
  - **AUD-001** — клиент не вызывает E2EE для голоса и экрана (`app/src` не использует `finch_protocol::e2ee`), хотя PROGRESS.md (этап 5) утверждает обратное.
  - Клиент не отправляет демонстрацию экрана: в `app/src/commands/screen.rs` есть только подписка, отписка и запрос ключевого кадра, захвата и отправки нет.
  - `quic::run_server`: `incoming.await?` — одно неудачное рукопожатие завершает цикл `accept` и сервер QUIC целиком.
  - `TofuVerifier::verify_tls12_signature` / `verify_tls13_signature` возвращают `assertion()` без проверки подписи. Сравнивается только отпечаток сертификата, поэтому сертификат можно переиспользовать без закрытого ключа (MITM).
  - TOFU молча доверяет первому подключению, хотя есть `TofuModal`.
  - REST-клиент: `reqwest` с `danger_accept_invalid_certs(true)`; сервер REST — обычный HTTP (`axum::serve` на TCP без TLS), то есть пароли и JWT идут открытым текстом.
  - JWT-секрет по умолчанию зашит в `config.rs`, и сервер стартует с ним без предупреждения.
  - `parse_quic_addr` в `app/src/commands/auth.rs`: QUIC-порт угадывается из HTTP-порта (8443/8080 → 4433, иначе тот же порт).
  - `EchoCanceller` — самописный адаптивный фильтр, а не `webrtc-audio-processing` AEC3, как заявлено в требовании 7.
  - Выбранные пользователем аудиоустройства не используются: движок всегда открывает устройства по умолчанию, хотя `get_audio_devices` существует.

- [ ] **Step 3: Ручной прогон в приложении.** Запустить `Run_Server.bat` и `Run_FinchSpeak.bat`, пройти: регистрация → вход → создание канала → сообщение → файл → голосовой канал (mute/deafen) → демонстрация экрана → перезапуск сервера (клиент переподключается?). Каждое расхождение — находка с шагами воспроизведения.

- [ ] **Step 4: Раздел «Расхождения с PROGRESS.md»** — каждое утверждение этапов 0–6 и требований 1–11 сверить с кодом. Расхождения перечислить с `файл:строка`.

- [ ] **Step 5: Сводка в начале отчёта** — таблица: ID, уровень, название, размер, «в фазе 3 / перенос / подпроект N». Пробелы распределить по подпроектам 2–6.

- [ ] **Step 6: Коммит**

```bash
git add docs/audit/2026-10-audit-report.md
git commit -m "docs(audit): add audit report for subproject 1"
```

- [ ] **Step 7: Точка согласования.** Показать отчёт пользователю и дождаться утверждения приоритетов. Без этого фаза 3 не начинается; фаза 2 может идти параллельно с ревью отчёта.

---

## Фаза 2 — стенд

### Task 2: Запуск сервера в процессе (`finch_server::launch`)

**Files:**
- Create: `crates/finch-server/src/launch.rs`
- Modify: `crates/finch-server/src/lib.rs` (добавить `pub mod launch;`)
- Modify: `crates/finch-server/src/quic.rs:73-108` (`run_server` принимает готовый `quinn::Endpoint`)
- Modify: `crates/finch-server/src/main.rs` (использует `launch`)
- Test: `crates/finch-server/tests/launch_test.rs`

**Interfaces:**
- Produces:
  - `pub async fn launch(config: ServerConfig) -> anyhow::Result<RunningServer>`. Инициализирует БД по `config.database.url`, привязывает HTTP (`TcpListener`) и QUIC (`Endpoint`) к `config.server.host` и портам. Порт `0` — свободный.
  - `pub struct RunningServer { pub http_addr: SocketAddr, pub quic_addr: SocketAddr, /* private: shutdown, task handles, endpoint */ }`
  - `impl RunningServer { pub async fn shutdown(self) }` — `endpoint.close(0u32.into(), b"shutdown")`, остановка axum через `with_graceful_shutdown`, ожидание задач.
  - `pub async fn run_server(endpoint: quinn::Endpoint, pool: SqlitePool, sessions: SessionManager, jwt_secret: String) -> Result<()>` — тело цикла не меняется, включая `incoming.await?` (это находка для фазы 3).

- [ ] **Step 1: Падающий тест** `launch_binds_ephemeral_ports_and_shuts_down`: конфиг с `host = "127.0.0.1"`, `http_port = 0`, `quic_port = 0`, `database.url = "sqlite::memory:"`, `upload_dir` во временной папке. Проверки:
  - `http_addr.port() != 0` и `quic_addr.port() != 0`;
  - `GET http://{http_addr}/api/channels` без токена возвращает 401;
  - после `shutdown().await` повторный `TcpListener::bind(http_addr)` успешен.

- [ ] **Step 2: Запуск** `cargo test -p finch-server --test launch_test` — FAIL: `unresolved import finch_server::launch`.

- [ ] **Step 3: Реализовать `launch`**, перенеся логику из `main.rs`. `main.rs` становится вызовом `launch` + `tokio::signal::ctrl_c()` + `shutdown()`. Логи `main.rs` сохранить.

- [ ] **Step 4: Запуск** `cargo test -p finch-server` — PASS (включая существующие `api_test`, `e2e_full_stack_test`, `load_test`, `quic_poc_test`).

- [ ] **Step 5: Ручная проверка:** `Run_Server.bat` стартует и слушает 8443/4433, Ctrl+C завершает процесс.

- [ ] **Step 6: Проверки Global Constraints и коммит** `refactor(server): add in-process launch API`.

### Task 3: `finch_audio::VoiceCapture`

**Files:**
- Create: `crates/finch-audio/src/capture.rs`
- Modify: `crates/finch-audio/src/lib.rs` (экспорт `VoiceCapture`, `VoiceCaptureConfig`, `CaptureOutput`)
- Modify: `crates/finch-audio/src/mixer.rs` (тестовые аксессоры, см. ниже)
- Modify: `crates/finch-audio/src/jitter.rs` (`pub fn buffered_len(&self) -> usize`)
- Test: в `capture.rs`, `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `pub struct VoiceCaptureConfig { pub channel_short_id: u32, pub bitrate_kbps: u32, pub vad_threshold_db: f32 }` с `Default`: `0`, `64`, `DEFAULT_VAD_THRESHOLD_DB` (−45.0) — те же значения, что сейчас в `audio_engine.rs`.
  - `pub struct CaptureOutput { pub packet: Option<Bytes>, pub vad: VadDecision }`
  - `impl VoiceCapture`:
    - `pub fn new(config: VoiceCaptureConfig) -> Result<Self, AudioOpusError>`
    - `pub fn push_playback_reference(&mut self, frame: &[f32; FRAME_SIZE_48K])` — проксирует в `EchoCanceller::push_reference`.
    - `pub fn process(&mut self, frame: &mut [f32; FRAME_SIZE_48K], sender_peer_id: u32, timestamp_ms: u32) -> CaptureOutput` — по порядку AEC → NS → VAD; если `vad.should_transmit()`, кодирует Opus и собирает пакет `MediaHeader::new_audio(channel_short_id, sender_peer_id, seq, timestamp_ms, vad.is_silence_packet(), false)` + payload; `seq` увеличивается через `wrapping_add` только при отправке. `frame` после вызова содержит обработанный сигнал (до Opus): по нему стенд меряет шум и эхо. Ошибка Opus логируется через `tracing::error!`, `packet = None`.
  - `AudioMixer::max_buffered_packets(&self) -> usize` — максимум `jitter_buffer.buffered_len()` по всем спикерам.
  - `AudioMixer::loss_rate(&self, peer_id: u32) -> Option<f64>`

- [ ] **Step 1: Падающие тесты** в `capture.rs`:
  - `voice_capture_matches_legacy_packet_layout`: тон 440 Гц, амплитуда 0.3, 5 кадров → в каждом `CaptureOutput.packet`: длина > 20; `MediaHeader::from_slice(&p[..20])` даёт `media_type == Audio`, `channel_id == 7`, `sender_peer_id == 42`, `sequence` 0,1,2,3,4, `timestamp == переданный`, `!is_silence()`, `!is_e2ee()`; payload декодируется `OpusVoiceDecoder` без ошибки.
  - `voice_capture_silence_not_transmitted`: 10 кадров нулей после 5 кадров тона → после окончания hangover (`HANGOVER_FRAME_COUNT` = 5) `packet == None` и `vad == VadDecision::Silence`.
  - `voice_capture_sequence_increments_only_on_send`: тон, тишина ×10, тон → у первого пакета после паузы `sequence` на 1 больше, чем у последнего пакета перед паузой.
- [ ] **Step 2: Запуск** `cargo test -p finch-audio capture` — FAIL: не компилируется (`VoiceCapture` не найден).
- [ ] **Step 3: Реализовать** `capture.rs` и аксессоры, перенеся логику из `audio_engine.rs:298-490` без изменения поведения.
- [ ] **Step 4: Запуск** `cargo test -p finch-audio` — PASS (новые + существующие 23).
- [ ] **Step 5: Проверки и коммит** `refactor(audio): extract VoiceCapture from audio engine`.

### Task 4: Крейт `finch-client-core`

**Files:**
- Create: `crates/finch-client-core/Cargo.toml` (deps: `finch-protocol`, `quinn`, `rustls`, `ring`, `reqwest` с теми же features, что в `app`, `tokio`, `bytes`, `dashmap`, `uuid`, `serde`, `serde_json`, `thiserror`, `tracing`; dev: `finch-server`, `tempfile`)
- Create: `crates/finch-client-core/src/{lib.rs,error.rs,tofu.rs,session.rs,api.rs}`
- Modify: `Cargo.toml` (members + `finch-client-core = { path = "crates/finch-client-core" }` в `workspace.dependencies`)
- Test: `crates/finch-client-core/tests/session_test.rs`

**Interfaces:**
- Consumes: `finch_server::launch::{launch, RunningServer}` (Task 2) — только в dev-тестах.
- Produces:
  - `error.rs`: `pub enum ClientError` (`thiserror`). Варианты: `Http { status: u16, body: String }`, `Network(String)`, `Quic(String)`, `Codec(#[from] CodecError)`, `AuthRejected { code: u16, message: String }`, `UnexpectedMessage(String)`, `Closed`.
  - `tofu.rs`: `TofuVerifier` и `create_client_config(hostname: &str, saved_fingerprints: Arc<DashMap<String, String>>) -> Result<quinn::ClientConfig, ClientError>` — переносятся из `app/src/connection.rs:20-133` **без изменений логики** (включая отсутствие проверки подписи — это находка).
  - `api.rs`:
    - `pub struct UserDto` — переносится из `app/src/state.rs` как есть; `ChannelInfoDto` и `MessagesHistoryDto` — из `app/src/commands/chat.rs` как есть.
    - `pub struct AuthResponse { pub token: String, pub user: UserDto }`
    - `pub struct ApiClient` с методами:
      - `new(base_url: &str) -> Result<Self, ClientError>` — `reqwest` с `danger_accept_invalid_certs(true)`, как сейчас;
      - `set_token(&mut self, token: String)`;
      - `register(&self, username: &str, password: &str, display_name: &str) -> Result<AuthResponse, ClientError>`;
      - `login(&self, username: &str, password: &str) -> Result<AuthResponse, ClientError>`;
      - `me(&self) -> Result<UserDto, ClientError>`;
      - `list_channels(&self) -> Result<Vec<ChannelInfoDto>, ClientError>`;
      - `create_channel(&self, name: &str, channel_type: &str) -> Result<ChannelInfoDto, ClientError>`;
      - `fetch_messages(&self, channel_id: Uuid, before: Option<i64>, limit: u32) -> Result<MessagesHistoryDto, ClientError>`;
      - `upload_file(&self, filename: &str, data: Vec<u8>, content_type: &str) -> Result<AttachmentInfo, ClientError>`;
      - `download_file(&self, id: Uuid) -> Result<Bytes, ClientError>`.
      
      Ответ не 2xx → `ClientError::Http { status, body }`. Пути и тела запросов — точно как в текущих `app/src/commands/{auth,chat}.rs`.
    - `pub fn resolve_quic_addr(server_url: &str) -> Result<(SocketAddr, String), ClientError>` — перенос `parse_quic_addr` без изменений.
  - `session.rs`:
    - `pub struct ConnectParams { pub quic_addr: SocketAddr, pub hostname: String, pub token: String }`
    - `pub struct Session { pub connection: quinn::Connection, pub control_tx: mpsc::Sender<ClientMessage>, pub user_id: Uuid, pub session_peer_id: u32, pub channels: Vec<ChannelMapping>, pub initial_state: ServerState }`
    - `impl Session { pub fn send_datagram(&self, packet: Bytes) -> Result<(), ClientError> }`
    - `pub enum SessionEvent { Control(ServerMessage), Datagram(Bytes), UniStream { header: MediaHeader, payload: Bytes }, ConnectionLost(String) }`
    - `pub async fn connect(params: ConnectParams, fingerprints: Arc<DashMap<String, String>>) -> Result<(Session, mpsc::Receiver<SessionEvent>), ClientError>`. Делает handshake, `Authenticate`, ждёт `Authenticated` (иначе `AuthRejected`/`UnexpectedMessage`) и запускает задачи, как в `start_quic_session`: отправка control, приём control, ping каждые 4 с, датаграммы, uni-стримы (лимит 10 МБ). Очередь событий ограничена 1024; датаграммы — `try_send` (переполнение → отброс + `tracing::debug!`), control, uni-стримы и `ConnectionLost` — `send().await`.

- [ ] **Step 1: Падающие тесты** (`session_test.rs`, сервер через `launch` с портами 0 и `sqlite::memory:`):
  - `register_login_and_connect`: `register("alice", "password_123", "Alice")` → `token` не пустой; `connect` → `session_peer_id != 0`, `initial_state.channels` не пуст.
  - `bad_password_returns_401`: `login("alice", "wrong")` → `Err(ClientError::Http { status: 401, .. })`.
  - `invalid_token_rejected_on_quic`: `connect` с токеном `"garbage"` → `Err(ClientError::AuthRejected { .. })`.
  - `control_events_survive_datagram_flood` (Review Focus 4): клиент B в голосовом канале не читает очередь событий, пока A отправляет 5000 аудио-датаграмм; затем A отправляет `SendMessage`. B, начав читать, получает `Control(NewMessage)` с этим текстом в течение 5 с.
- [ ] **Step 2: Запуск** `cargo test -p finch-client-core` — FAIL: не компилируется.
- [ ] **Step 3: Реализовать крейт**, перенося код из `app/src`; Tauri-вызовы `emit` заменяются отправкой `SessionEvent`.
- [ ] **Step 4: Запуск** `cargo test -p finch-client-core` — PASS.
- [ ] **Step 5: Проверки и коммит** `refactor(client): extract Tauri-free finch-client-core`.

### Task 5: Перевод приложения на `finch-client-core` и `VoiceCapture`

**Files:**
- Modify: `app/Cargo.toml` (dep `finch-client-core`)
- Modify: `app/src/connection.rs` — остаётся только `start_quic_session(...)` с прежней сигнатурой: вызывает `finch_client_core::session::connect`, сохраняет `control_tx` и `connection` в `AppState`, `session_peer_id`, эмитит `authenticated` и запускает задачу-адаптер `SessionEvent` → прежние Tauri-события с прежними именами и JSON (`new-message`, `message-history`, `user-joined-voice`, `user-left-voice`, `user-voice-state-changed`, `screen-share-started`, `screen-share-stopped`, `presence-update`, `keyframe-requested`, `ping-rtt`, `server-error`, `connection-lost`, `user-speaking`). Маршрутизация датаграмм в `mixer` и `assembler` и uni-стримов в `video_channel` — как сейчас.
- Modify: `app/src/commands/auth.rs`, `app/src/commands/chat.rs` — HTTP через `ApiClient` (хранится в `AppState` как `Arc<RwLock<Option<ApiClient>>>` вместо `http_client`); сигнатуры Tauri-команд и возвращаемые DTO не меняются.
- Modify: `app/src/state.rs` (`UserDto` реэкспорт из `finch_client_core::api`)
- Modify: `app/src/audio_engine.rs` — блок DSP `298-490` заменяется на `VoiceCapture::process` + `push_playback_reference`; события `local-speaking` по `CaptureOutput.vad`; отправка пакета — как сейчас через `state.quic_connection`.

**Interfaces:**
- Consumes: всё из Task 3 и Task 4.
- Produces: фронтенд не меняется (`app/frontend` не трогаем).

- [ ] **Step 1: Перевести модули.** Существующие тесты `app` (`test_audio_engine_lifecycle`, `test_ringbuf_transfer`) — регрессионная защита.
- [ ] **Step 2: Запуск** проверок Global Constraints — всё зелёное.
- [ ] **Step 3: Ручная проверка** (Review Focus 1): сервер + клиент, вход, сообщение, голосовой канал на устройстве 44.1 кГц/стерео (или текущем по умолчанию): «слышу собеседника» через второй экземпляр клиента на той же машине, индикатор речи, mute/deafen. Поведение такое же, как до рефакторинга.
- [ ] **Step 4: Коммит** `refactor(app): use finch-client-core and VoiceCapture`.

### Task 6: Каркас `finch-testbed` и сценарии аккаунта и чата

**Files:**
- Create: `crates/finch-testbed/Cargo.toml` (`publish = false`; deps: `finch-server`, `finch-client-core`, `finch-audio`, `finch-protocol`, `finch-screen`, `tokio`, `bytes`, `uuid`, `anyhow`, `tempfile`, `hound`, `rand`, `jsonwebtoken` (для поддельных и просроченных токенов; формат claims — как в `crates/finch-server/src/api/auth.rs`), `tracing-subscriber`)
- Create: `crates/finch-testbed/src/lib.rs`, `src/server.rs`, `src/client.rs`
- Modify: `Cargo.toml` (members)
- Test: `crates/finch-testbed/tests/auth.rs`, `crates/finch-testbed/tests/chat.rs`

**Interfaces:**
- Consumes: `launch`/`RunningServer` (Task 2), `ApiClient`/`connect`/`Session`/`SessionEvent` (Task 4).
- Produces:
  - `pub struct TestServer { pub http_url: String, pub quic_addr: SocketAddr, /* private: TempDir, RunningServer, ServerConfig */ }`
  - `TestServer::start() -> anyhow::Result<Self>` — `127.0.0.1`, порты 0, БД `sqlite://{tmp}/finch.db?mode=rwc`, `upload_dir = {tmp}/uploads`, `max_file_size_mb = 1`.
  - `TestServer::restart(&mut self) -> anyhow::Result<()>` — `shutdown` и `launch` на тех же портах и той же БД.
  - `TestServer::stop(&mut self)`.
  - `pub struct TestClient { pub api: ApiClient, pub session: Session, pub user: UserDto, /* private: events, буфер датаграмм */ }`
  - `TestClient::register(server: &TestServer, username: &str) -> anyhow::Result<Self>` — пароль `"password_123"`, `display_name = username`.
  - `TestClient::send(&self, msg: ClientMessage) -> anyhow::Result<()>`
  - `TestClient::expect_control<T>(&mut self, timeout: Duration, pred: impl FnMut(&ServerMessage) -> Option<T>) -> anyhow::Result<T>` — пропускает несовпавшие события; датаграммы откладывает в буфер; по таймауту ошибка с описанием последних 10 событий.
  - `TestClient::expect_no_control(&mut self, within: Duration, pred: impl FnMut(&ServerMessage) -> bool) -> anyhow::Result<()>`
  - `TestClient::drain_datagrams(&mut self) -> Vec<Bytes>` — неблокирующий сбор из буфера и очереди.
  - `TestClient::channel(&self, kind: ChannelType) -> (Uuid, u32)` — первый канал нужного типа из `session.initial_state.channels`.

- [ ] **Step 1: Тесты `auth.rs`:**
  - `register_and_login`;
  - `wrong_password_is_401`;
  - `forged_jwt_rejected_on_rest_and_quic` — токен, подписанный другим секретом: REST `me()` → 401, `connect` → `AuthRejected`;
  - `expired_jwt_rejected` — токен с `exp` в прошлом, подписанный секретом по умолчанию.
- [ ] **Step 2: Тесты `chat.rs`:**
  - `message_broadcast_to_all` — A, B, C; A шлёт `SendMessage` в текстовый канал; B и C получают `NewMessage` с тем же `content` и `author_id == A.user.id` за 2 с;
  - `history_persisted_in_order` — 3 сообщения, `fetch_messages(limit 50)` возвращает их в порядке отправки;
  - `file_upload_download_roundtrip` — 100 КБ случайных байт (seed 42), скачанные равны загруженным;
  - `file_over_limit_rejected` — 1.5 МБ → `ClientError::Http { status }` с `status` 413 или 400; 2xx или 5xx — находка.
- [ ] **Step 3: Запуск** `cargo test -p finch-testbed` — FAIL: каркас не реализован.
- [ ] **Step 4: Реализовать** `server.rs`, `client.rs`.
- [ ] **Step 5: Запуск** `cargo test -p finch-testbed`. Красный сценарий из-за бага в продукте не чинится здесь: он записывается в отчёт и получает `#[ignore = "AUD-NNN"]`. Красный из-за бага стенда — чинится.
- [ ] **Step 6: Проверки и коммит** `test(testbed): add harness with auth and chat scenarios`.

### Task 7: Сценарии голоса, экрана и E2EE

**Files:**
- Test: `crates/finch-testbed/tests/voice_signaling.rs`, `tests/screen.rs`, `tests/e2ee.rs`

**Interfaces:**
- Consumes: `TestServer`, `TestClient` (Task 6); `MediaHeader`, `encrypt_media_payload`/`decrypt_media_payload` (`finch-protocol`); `slice_p_frame` (`finch-screen`).

- [ ] **Step 1: `voice_signaling.rs`:**
  - `join_leave_presence` — A входит в голосовой канал → B получает `UserJoinedVoice { peer_id == A.session_peer_id, .. }`; A выходит → `UserLeftVoice`.
  - `muted_client_state_broadcast` — A `UpdateVoiceState { muted: true, deafened: false }` → B получает `UserVoiceStateChanged { muted: true, .. }`.
  - `deafened_client_receives_no_audio` — B deafen; A шлёт 50 аудио-датаграмм; `B.drain_datagrams()` пуст через 500 мс; C (не оглушён) получил ≥ 45.
  - `sender_peer_id_is_rewritten` — A шлёт пакет с `sender_peer_id = 999_999`; у B заголовок содержит `A.session.session_peer_id`.
- [ ] **Step 2: `screen.rs`:**
  - `screen_share_keyframe_and_deltas` — A `StartScreenShare`, B `SubscribeScreenShare`. A отправляет IDR (64 КБ) через uni-стрим с `MediaHeader::new_video_key`, затем дельта-кадр 5000 байт через `slice_p_frame` датаграммами. B получает `UniStream` с payload, равным IDR, и все фрагменты дельты; `FrameAssembler` B собирает кадр, равный исходному.
  - `keyframe_request_forwarded` — B `RequestKeyframe { target_peer_id: A }` → A получает `KeyframeRequested` за 1 с.
- [ ] **Step 3: `e2ee.rs`:** `encrypted_audio_survives_sfu` — ключ `[7u8; 32]`; A шифрует Opus-payload (`encrypt_media_payload` с заголовком, где `sender_peer_id` = свой `session_peer_id`). B с тем же ключом расшифровывает в исходный payload; C с ключом `[8u8; 32]` получает `Err`.
- [ ] **Step 4: Запуск** `cargo test -p finch-testbed` — PASS или пометки `#[ignore = "AUD-NNN"]` с находками в отчёте (правило из Task 6 Step 5).
- [ ] **Step 5: Проверки и коммит** `test(testbed): voice signaling, screen share and E2EE scenarios`.

### Task 8: Сценарии устойчивости

**Files:**
- Test: `crates/finch-testbed/tests/resilience.rs`

- [ ] **Step 1: Тесты:**
  - `client_reconnects_after_server_restart` (Review Focus 5): A и B регистрируются, A шлёт сообщение; `server.restart()`; оба переподключаются через `connect` с прежним токеном; `fetch_messages` возвращает сообщение; A снова в голосовом канале → B получает `UserJoinedVoice`. Автоматического переподключения в клиенте сейчас нет — стенд переподключает вручную, а отсутствие авто-переподключения записывается в отчёт как находка «Важно».
  - `abrupt_disconnect_cleans_presence` (Review Focus 3): A и B в голосовом канале; `A.session.connection.close(...)` без `LeaveVoice` → B получает `UserLeftVoice` за ≤ 20 с (таймаут простоя 15 с); затем новый клиент C входит в канал и получает ответ сервера за 2 с (нет дедлока).
  - `server_survives_failed_handshake` (Review Focus 2): UDP-сокет шлёт на `quic_addr` мусорный Initial-пакет 1200 байт; затем открывается и бросается настоящий `quinn`-коннект с ALPN `b"wrong"`; после этого `TestClient::register` + `connect` успешен за 5 с.
- [ ] **Step 2: Запуск, пометки** `#[ignore = "AUD-NNN"]` для красных из-за продукта.
- [ ] **Step 3: Проверки и коммит** `test(testbed): resilience scenarios`.

### Task 9: Аудио-утилиты, фикстуры и метрики

**Files:**
- Create: `crates/finch-testbed/src/audio.rs`, `crates/finch-testbed/src/metrics.rs`
- Create: `crates/finch-testbed/fixtures/speech_{1,2,3}.wav` (16 кГц моно 16 бит, суммарно ≤ 15 с), `crates/finch-testbed/fixtures/ATTRIBUTION.md`

**Interfaces:**
- Produces:
  - `audio::load_speech_48k(name: &str) -> anyhow::Result<Vec<f32>>` — читает WAV 16 кГц через `hound`, апсемплинг в 48 кГц через `finch_audio::InputResampler::new(16_000)`.
  - `audio::white_noise(len: usize, rms_dbfs: f32, seed: u64) -> Vec<f32>`, `pink_noise(..)` (фильтр Пола Келлета), `babble(len, rms_dbfs, seed) -> Vec<f32>` (сумма 3 фикстур со случайными сдвигами), `chirp(len: usize, f0: f32, f1: f32) -> Vec<f32>`, `mix_at_snr(speech: &[f32], noise: &[f32], snr_db: f32) -> Vec<f32>`.
  - `audio::write_wav_48k(path: &Path, samples: &[f32])` — отладочный вывод в `target/testbed-out/` при падении теста.
  - `metrics::rms_dbfs(x: &[f32]) -> f32`
  - `metrics::snr_db(signal_segment: &[f32], noise_segment: &[f32]) -> f32` = `10·log10((P(signal_segment) − P(noise_segment)) / P(noise_segment))`, числитель ограничен снизу `1e-12`.
  - `metrics::segmental_snr_db(x: &[f32], clean: &[f32], noise_floor: f32) -> f32` — среднее по 20-мс кадрам, где `rms_dbfs(clean_frame) > −40`, значения `10·log10((P(x_frame) − noise_floor) / noise_floor)`, каждое обрезано до [−10, 35].
  - `metrics::delay_samples(reference: &[f32], recorded: &[f32], max_lag: usize) -> usize` — кросс-корреляция, аргмакс.
  - `metrics::erle_db(echo_in: &[f32], residual_out: &[f32]) -> f32` = `10·log10(P(echo_in) / P(residual_out))`.

- [ ] **Step 1: Получить фикстуры.** Источник — 3 фрагмента LibriSpeech (CC BY 4.0) из датасета Hugging Face `hf-internal-testing/librispeech_asr_dummy` (~9 МБ parquet). **Перед скачиванием спросить пользователя** (имя файла, источник, размер). Скачанное положить в отдельную пустую папку во временной директории, извлечь скриптом из другой папки, обрезать до ≤ 5 с каждый, сохранить 16 кГц 16 бит. В `ATTRIBUTION.md` — ID фрагментов, авторы чтения из метаданных, лицензия и ссылка.
- [ ] **Step 2: Падающие тесты метрик** (в `metrics.rs`, синтетика):
  - `snr_of_known_mix` — `mix_at_snr(tone, white, 5.0)` → `snr_db` ∈ [4.5, 5.5];
  - `delay_detected` — чирп, сдвинутый на 2400 отсчётов → `delay_samples == 2400`;
  - `erle_of_attenuated_copy` — `residual = 0.1 × echo` → `erle_db` ∈ [19.9, 20.1];
  - `fixtures_load_at_48k` — `load_speech_48k("speech_1")`: длина ≈ 3 × исходной (±1 кадр), `rms_dbfs > −35`.
- [ ] **Step 3: Запуск** — FAIL. **Step 4:** реализовать. **Step 5:** запуск — PASS.
- [ ] **Step 6: Проверки и коммит** `test(testbed): audio fixtures and signal metrics` (`git add` фикстур и атрибуции; проверить `du -ch fixtures` < 1 МБ).

### Task 10: Голосовые метрики

**Files:**
- Create: `crates/finch-testbed/src/voice.rs`
- Test: `crates/finch-testbed/tests/voice_quality.rs`
- Create: `crates/finch-testbed/BASELINE.md`

**Interfaces:**
- Consumes: `VoiceCapture`, `AudioMixer::{on_audio_packet, mix_frame, max_buffered_packets, loss_rate}` (Task 3), `TestClient` (Task 6), `audio`/`metrics` (Task 9).
- Produces:
  - `pub struct VoiceRun { pub output: Vec<f32>, pub speaking_events: Vec<(usize /*кадр*/, u32, bool)>, pub received_headers: Vec<MediaHeader>, pub max_buffered_packets: usize, pub loss_rate: Option<f64> }`
  - `pub async fn run_voice_link(sender: &TestClient, receiver: &mut TestClient, input: &[f32], tail_frames: usize) -> anyhow::Result<VoiceRun>`. Оба клиента уже в одном голосовом канале. `tokio::time::interval(20 мс)` с `MissedTickBehavior::Burst`. Каждый тик:
    1. следующий кадр входа (после конца — нули ещё `tail_frames` кадров) → `VoiceCapture::process(sender_peer_id = sender.session.session_peer_id, timestamp_ms = кадр × 20)` → при `Some(packet)` → `sender.session.send_datagram`;
    2. `receiver.drain_datagrams()` → `mixer.on_audio_packet(Instant::now())`, заголовки в `received_headers`;
    3. `mixer.mix_frame` → в `output`, события в `speaking_events`.

- [ ] **Step 1: Тесты `voice_quality.rs`** (пороги — из Global Constraints):
  - `end_to_end_latency_under_120ms` — 200 мс тишины + чирп 100 мс (300→3000 Гц, амплитуда 0.5) + 1 с тишины. `delay_samples(input, output, 48_000) × 1000 / 48_000 ≤ 120`.
  - `noise_suppression_improves_snr` — для каждого шума (white, pink, babble): 1 с только шума, затем `speech_1` + шум при SNR 5 дБ. Прогон через `VoiceCapture::process` напрямую (отклонение 3). Сравнить на выходе: `snr_db(речевой участок, шумовой участок за последние 500 мс первой секунды)` минус то же на входе — **≥ 10 дБ**; `segmental_snr_db(out) ≥ segmental_snr_db(in)`. Значения записать в `BASELINE.md`.
  - `echo_cancelled_after_convergence` — дальний сигнал `speech_2` → в `push_playback_reference` покадрово; вход = дальний, задержанный на 1920 отсчётов (40 мс) и ослабленный в 0.5 раза (−6 дБ). Первые 2 с пропускаются; затем 2 с без ближней речи: `erle_db(вход, выход) ≥ 20`. Затем 2 с с ближней речью `speech_3` — `rms_dbfs` выхода не ниже `rms_dbfs(speech_3) − 6` (AEC не съедает ближнюю речь).
  - `silence_and_noise_do_not_light_speaking` — 3 с нулей, затем 3 с розового шума −35 dBFS → в `speaking_events` нет `(_, _, true)`; каждый `received_headers` имеет `is_silence() == true` (или пакетов нет).
- [ ] **Step 2: Запуск** — FAIL (нет `voice.rs`). **Step 3:** реализовать `voice.rs`. **Step 4:** запуск.
- [ ] **Step 5: Порог не пройден → находка «Важно» в отчёте**, тест получает `#[ignore = "AUD-NNN"]`, измеренное значение — в `BASELINE.md`. Порог не менять.
- [ ] **Step 6: `BASELINE.md`** — таблица: метрика, порог, измерено, дата, коммит.
- [ ] **Step 7: Проверки и коммит** `test(testbed): voice quality metrics and baseline`.

### Task 11: Плохая сеть

**Files:**
- Create: `crates/finch-testbed/src/netem.rs`
- Test: `crates/finch-testbed/tests/network_impairment.rs`

**Interfaces:**
- Produces:
  - `pub struct NetemConfig { pub loss: f64, pub delay_ms: (u64, u64), pub seed: u64 }`
  - `pub struct UdpImpairmentProxy { pub listen_addr: SocketAddr }`
  - `UdpImpairmentProxy::start(upstream: SocketAddr, cfg: NetemConfig) -> anyhow::Result<Self>` — UDP-прокси клиент ↔ сервер. Каждый пакет в обе стороны с вероятностью `loss` отбрасывается, иначе отправляется через случайную задержку из `delay_ms` в отдельной задаче, поэтому перестановка пакетов возникает естественно. Остановка — при `Drop`.

- [ ] **Step 1: Тесты:**
  - `proxy_drops_and_delays` (юнит, локальные UDP-сокеты): 1000 пакетов, `loss = 0.05`, задержка (0, 60) → доставлено 930…980, есть хотя бы одна перестановка порядка.
  - `voice_under_loss_and_jitter` — B подключается через прокси (`loss = 0.05`, задержка (0, 60) мс, seed 42); вход = 200 мс тишины + чирп + `speech_1`. Проверки:
    - нет паники;
    - `delay_samples × 1000 / 48_000 ≤ 200`;
    - `max_buffered_packets ≤ MAX_BUFFER_PACKETS` (6);
    - `loss_rate > 0`;
    - на речевом участке нет серии из более чем 3 подряд нулевых кадров (PLC закрывает пропуски).
- [ ] **Step 2: Запуск** — FAIL. **Step 3:** реализовать `netem.rs`. **Step 4:** запуск; красное из-за продукта → находка + `#[ignore]` + `BASELINE.md`.
- [ ] **Step 5: Проверки и коммит** `test(testbed): network impairment proxy and scenario`.

### Task 12: Ручной чек-лист и запуск одной командой

**Files:**
- Create: `docs/audit/manual-voice-checklist.md`
- Modify: `README.md` (раздел «Тестирование»: `cargo test -p finch-testbed`, где `BASELINE.md`, где отладочные WAV)

- [ ] **Step 1: Чек-лист** — 5 пунктов из спека (раздел 4) с ожидаемым результатом у каждого. Пункты, которых нет в приложении («слышу себя», выбор устройств), помечены «недоступно — AUD-NNN, подпроект 2» и заменены доступной проверкой: второй экземпляр клиента на той же машине.
- [ ] **Step 2: Прогон** `cargo test -p finch-testbed` — все тесты зелёные или `ignored` с `AUD-NNN`; список ignored совпадает с находками в отчёте.
- [ ] **Step 3: Коммит** `docs(audit): manual voice checklist and testbed docs`.

---

## Фаза 3 — исправления

Начинается только после того, как пользователь утвердил приоритеты в отчёте (Task 1 Step 7).

### Task 13…N: Исправление находки AUD-NNN (по одному Task на находку)

Порядок: сначала «Критично» по убыванию серьёзности, затем «Важно». Список и порядок берутся из утверждённой сводки отчёта. Для каждой находки:

**Files:** указаны в находке (`Где:`); тест — в `finch-testbed/tests/` (интеграционное поведение) или в `#[cfg(test)]` модуле файла (локальная логика).

- [ ] **Step 1: Падающий тест.** Если уже есть `#[ignore = "AUD-NNN"]`, снять `#[ignore]`. Иначе написать тест, воспроизводящий шаги из находки.
- [ ] **Step 2: Запуск** — FAIL по причине из находки (не из-за ошибки компиляции теста).
- [ ] **Step 3: Исправление** по «Предлагаемому исправлению» находки. Если оно меняет протокол или API, согласованные в спеке, — остановиться и согласовать с пользователем.
- [ ] **Step 4: Запуск** теста — PASS, затем `cargo test --workspace` — PASS.
- [ ] **Step 5: Проверки Global Constraints и коммит** `fix(<crate>): <кратко> (AUD-NNN)`.
- [ ] **Step 6: Отметить в отчёте** статус находки: «исправлено, коммит <sha>».

Ожидаемые крупные пункты (уточняются отчётом; каждый — отдельный Task с планом из шагов выше, а L-размер — со своим мини-планом, согласованным перед началом):
- AUD-001 E2EE на клиенте: ключ комнаты и его доставка. Обмен ключом по протоколу — **это изменение протокола**, согласовать отдельно.
- Проверка подписи в `TofuVerifier` (верификация через `rustls::crypto::verify_tls13_signature` провайдера `ring`).
- `run_server` не падает на сбойном `incoming`.
- TLS для REST или перевод REST в QUIC — **решение пользователя**, может быть перенесено в подпроект 6.

### Task последний: Закрытие подпроекта

**Files:**
- Modify: `PROGRESS.md`, `docs/audit/2026-10-audit-report.md`

- [ ] **Step 1: PROGRESS.md** — убрать неверные утверждения из раздела «Расхождения»; метрики из `BASELINE.md`; число тестов по факту `cargo test --workspace`.
- [ ] **Step 2: Отчёт** — у каждой находки «Критично» и «Важно» статус «исправлено» или «перенесено пользователем → подпроект N».
- [ ] **Step 3: Попросить пользователя пройти** `docs/audit/manual-voice-checklist.md` и записать результат в отчёт.
- [ ] **Step 4: Проверки Global Constraints и коммит** `docs: close subproject 1 (audit, testbed, fixes)`.
