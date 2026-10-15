# Handoff Bugfixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans task-by-task. Human approval gates were explicitly waived by the owner; do not ask again.

**Goal:** Закрыть открытые баги HANDOFF с воспроизводящими проверками и честными статусами.

**Architecture:** Существующие SolidJS stores, Tauri commands и headless testbed остаются границами подсистем. Изменения протокола ограничены корреляцией сообщений; медиаформат 20 байт сохраняется. Аудио использует выделенный worker и lock-free callbacks.

**Tech Stack:** Rust/QUIC/rustls/SQLite/Tauri, SolidJS/TypeScript/Vitest, CPAL/rubato/RNNoise/Sonora, OpenH264/WebCodecs.

**Spec:** `docs/superpowers/specs/2026-10-15-handoff-bugfixes-design.md`; находки и исходные пороги — `docs/HANDOFF.md` и исходный спек аудита.

## Global Constraints

- Windows clients, one self-hosted server, around 50 users; no E2EE.
- MediaHeader stays exactly 20 bytes; authenticated SFU sender rewrite stays in place.
- No production unwrap/expect/todo in network/server/audio code.
- No callback mutexes or allocations; bounded queues and shutdown.
- Original audio thresholds stay unchanged; positive speech and real delivered traffic must be checked.

## Review Focus

- События во время восстановления снимка и история во время смены канала.
- Два входа одного пользователя; logout/замена сессии во время reconnect.
- Пароль не отправляется до доверия сертификату, pin mismatch и redirect.
- Горячее отключение устройства, неподдерживаемый sample format, переполнение эталонной очереди.
- Новый зритель экрана, потерянный IDR, крупный ключевой кадр, остановка всех worker при logout.

## Tasks

- [x] Фронтенд AUD-010/053/054/055/056: `chatStore`, `types`, API/Sidebar, Vitest + `tsc` в build; воспроизведение → исправление → тесты/сборка.
- [x] События AUD-019/020 и mute/admin AUD-033/037: `sessionStore`, `sessionEvents`, `App`, панели; snapshot, profile refresh, reconnect history и UI-тесты.
- [x] Сервер AUD-030/038/039: `session`, `api/auth`, `quic`; параллельная регистрация, сохранность второго входа и отзыв REST/QUIC.
- [x] N+1 AUD-040 и замечания стенда #7/#8/#10: реальный подсчёт SQLite SELECT, независимые чирпы и UDP recovery; проверки независимыми исполнителями.
- [ ] AUD-031: `app/src/connection.rs`; закрыть старый conn и подавить события старого поколения; реальный QUIC-тест.
- [ ] Ограничить graceful drain: `launch.rs`, stalled-task regression; общий срок 5 секунд.
- [ ] AUD-021 DSP: Sonora adapter в `finch-audio/aec.rs`; существующий ERLE/double-talk тест плюс focused AEC tests.
- [ ] AUD-021 ввод/вывод: `app/audio_engine.rs`, bounded actual-playout reference/resampler; тесты тишины/deafen/overflow.
- [ ] AUD-024: выбор устройств в audio engine/commands/UI; восстановление ошибок, ручная аппаратная проверка.
- [ ] AUD-050/#9: исследовать и проверить VAD на шуме и настоящей речи; не подменять результат отсутствием пакетов.
- [ ] AUD-003: HTTPS server + strict TOFU API client + desktop trust-before-password; обновить стенд и TLS-регрессии.
- [ ] AUD-032: CSP, совместимая с реальными Tauri IPC, WebCodecs и загрузкой приложения; проверить сборку/интерфейс.
- [ ] AUD-018/035: capture lifecycle, IDR limit/concurrency, WebCodecs viewer, keyframe requests, screen audio; protocol и UI tests, реальный захват по доступности среды.
- [ ] AUD-036: UUID корреляции в protocol/API/SQLite/history; отказ сервера откатывает правильный pending, включая одинаковые тексты и reconnect.
- [ ] Проверить намеренный runtime-liveness guard стенда: сохранить защиту теста от deadlock; заменить утечку только при сохранении свойства.
- [ ] Общая проверка workspace/frontend, ревью текущего diff, ручные доступные сценарии, обновление HANDOFF/PROGRESS/audit с точными ограничениями.

## Execution ledger

- Ruling: сохранять рабочий checkout, создать `codex/handoff-bugfixes` — исходная ветка оказалась `main`, не указанная HANDOFF `audit-and-testbed`.
- Ruling: исключить целиком generated `dist` из Git, сохранять сборку на диске — исходно отслеживались лишь HTML/CSS/logo без JS.
- Ruling: Rust установлен локально в `.toolchains`, npm-зависимости установлены — исходная среда не имела доступного Rust и зависимостей frontend.
- Ruling: реальные localhost QUIC tests запускаются вне сетевой песочницы — внутри неё UDP-проверки зависают.
- Ruling: RNNoise probability 0,5 не считать исправлением — измерения показывают уверенную ложную классификацию pink noise.
