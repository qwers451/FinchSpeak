# FinchSpeak — передача работы второму разработчику

- **Дата:** 2026-10-15
- **Текущая ветка:** `codex/handoff-bugfixes`, создана от фактической локальной `main`, HEAD `e6f4c97`.
- **Git:** новых коммитов и push в этом продолжении нет. Изменения частично staged (фронтенд и исключение `dist`), частично unstaged; новые Rust-тесты и документы также есть среди untracked. Не считать индекс полным набором изменений.
- **Состояние:** значительная часть фронтенда, сессий, авторизации и стенда исправлена и проверена отдельными прогонами. Работа прервана во время AEC3/HTTPS/VAD; **текущий workspace не подтверждён как зелёный и содержит несовместимый промежуточный HTTPS-переход**. Подробности и порядок продолжения — раздел 0.

Этот документ — единая точка входа: что сделано, какие ошибки ещё открыты, что запланировано дальше. Подробности по каждой находке — в [отчёте аудита](audit/2026-10-audit-report.md); исходный замысел — в [спеке подпроекта 1](superpowers/specs/2026-10-15-audit-and-testbed-design.md) и [плане](superpowers/plans/2026-10-15-audit-and-testbed.md).

---

## 0. Актуальное продолжение от 2026-10-15

**Раздел 0 имеет приоритет над статусами разделов 1–7: ниже сохранена исходная передача, где фронтенд ещё указан неисправленным, а часть уже выполненных пунктов — открытой.** Число «140 тестов» ниже тоже историческое, а не результат полного прогона текущего дерева.

### 0.1. Запрос и решения владельца

- Исправить все баги из HANDOFF, не делать предположительных исправлений.
- Разрешена установка всех необходимых инструментов и зависимостей.
- После согласования первого блока владелец явно разрешил **все необходимые исправления без дальнейших согласований дизайна и планов**. Не запрашивать эти согласования повторно.
- Подход AEC: AEC3/Sonora, эталон фактически воспроизведённого звука, сохранение исходных порогов. E2EE по-прежнему не добавляется.
- Затем владелец прервал реализацию и попросил описать все изменения здесь. Исполнители остановлены; по их финальным отчётам активных Cargo-сессий не осталось. **Оставшиеся баги не объявлены закрытыми.**

Новые рабочие документы: [спек исправлений](superpowers/specs/2026-10-15-handoff-bugfixes-design.md), [план и журнал решений](superpowers/plans/2026-10-15-handoff-bugfixes.md). Это планируемый итог, не свидетельство выполнения всех пунктов.

### 0.2. Срочно учесть перед запуском

1. **AUD-003, HTTPS пока не завершён.** `finch-client-core/src/api.rs` уже отвергает `http://` и проверяет HTTPS/сертификаты; `finch-server/src/launch.rs` всё ещё обслуживает обычный HTTP. Поэтому текущие клиент и сервер несовместимы, а старый TestServer с HTTP-адресом больше не подходит для новых клиентов. Серверная TLS-обвязка, вход Tauri с проверкой сертификата до пароля и миграция стенда не реализованы. После изменения клиента компиляция/тесты этого блока не запускались.
2. **AUD-021, AEC3 не завершён.** Первоначальный Sonora-вариант прошёл основной ERLE-тест, но после следующих изменений он не перепроверен. Последний прогон AEC unit-тестов: **6 passed, 1 failed** — `test_aec_double_talk_preserves_speech`. В production `aec.rs` остались два запрещённых проектом `expect()` на установке stream delay. Исправить их, затем повторить unit/ERLE/double-talk проверки.
3. **AUD-050 остаётся открытым.** Earshot добавлен в зависимости, но в production pipeline ещё не включён. Тест на шум усилен положительной проверкой настоящей речи, остаётся ignored и воспроизводит баг.
4. `cargo test --workspace` и Clippy для итогового дерева не выполнены. Не коммитить/сливать как полностью проверенный результат. `PROGRESS.md` и исходный отчёт аудита ещё не обновлены и могут содержать прежние заявления о готовности.

### 0.3. Выполненные изменения фронтенда

Последний подтверждённый прогон: **27 frontend-тестов прошли**, `npm run build` прошёл с обязательной проверкой TypeScript. Это проверка фронтенда, не всего приложения после незавершённого HTTPS-перехода.

| Находка / область | Изменение | Файлы / проверка |
|---|---|---|
| AUD-053 | Импортирован `currentUser`; отправка использует текущего автора | `stores/chatStore.ts`, `chatStore.test.ts` |
| AUD-055 | Внутренняя модель сообщения использует `id`; живой `message_id` нормализуется на входе; типы истории и live разделены | `types.ts`, `services/api.ts`, `chatStore.ts`; два live-сообщения подряд |
| AUD-010 | Убран лишний reverse истории; добавлен `channel_id` к сообщениям истории | `chatStore.ts`; хронологическая история |
| AUD-054 | Исправлены импорты `api` / `loadChannels`, ошибки создания канала видимы | `Sidebar.tsx`; создание канала через DOM-тест |
| AUD-019 | Подключены authenticated/initial_state, presence, remote mute/deafen, connection-lost/reconnecting/reconnected/reconnect-failed и server-error; баннеры соединения и ошибок | новые `sessionStore.ts`, `notificationStore.ts`, `services/sessionEvents.ts`, изменения `App.tsx` |
| AUD-019, восстановление | При перезагрузке UI запрашивается снимок действующей Rust-сессии. Для новых онлайн-пользователей запрашиваются профили. События во время снимка повторно применяются поверх него. После reconnect подгружается история выбранного чата | `sessionEvents.test.ts`, `App.test.tsx`; серверный `/api/state` и Tauri-команда описаны ниже |
| AUD-020 | Общие voice states фильтруются по активной комнате, в sidebar участники показаны под своей комнатой. Локальный peer исключён из повторной карточки | `voiceStore.ts`, `Sidebar.tsx`, `VoicePanel.tsx` |
| AUD-033 | После deafen восстанавливается предыдущий mute в текущем UI-сеансе; состояние меняется после успешной команды | `voiceStore.ts`; оба исходных значения mute проверены |
| AUD-037 | Кнопки создания каналов доступны только admin; серверные проверки прав сохранены | `Sidebar.tsx`, DOM-тест для member |
| AUD-056 | `build` теперь `tsc --noEmit && vite build`; исправлены исходные TS-ошибки, CSS `accent-color`, тип `Set<number>`, Vite env declarations | `package.json`, `VoicePanel.tsx`, `voiceStore.ts`, новый `vite-env.d.ts` |
| AUD-036, частично | При отказе invoke удаляется только соответствующий optimistic message, ошибка показывается пользователю. UUID вместо `Date.now()` исключает совпадение локальных ID | `chatStore.ts`; **асинхронный отказ сервера ещё не связан с конкретной отправкой** |
| Дополнительные гонки | Поздняя история не перезаписывает новый выбранный канал; live-сообщения во время загрузки не теряются; повторная история сохраняет порядок; live echo не оставляет дубликат pending при пересечении с историей | `chatStore.test.ts`; guard поколения запроса, объединение/сортировка и учёт уже полученных live ID |
| Жизненный цикл UI | Подписки готовы до показа формы входа; очистка работает при размонтировании во время async-регистрации и при частичном отказе подписок | `App.tsx`, `sessionEvents.ts`, DOM-тест поздней очистки |
| Logout | Очищаются профиль, каналы, сообщения, участники, TOFU и состояние сессии; локальный выход завершается и при ошибке удалённого отзыва, ошибка видима | `authStore.ts`, `authStore.test.ts`; mute/deafen не сбрасываются лишь на фронтенде, поскольку Rust сохраняет эти флаги |

Подключены Vitest и happy-dom, отдельный `vitest.config.ts` с Solid plugin без HMR. Добавлены `App.test.tsx`, `sessionEvents.test.ts`, `chatStore.test.ts`, `voiceStore.test.ts`, `authStore.test.ts`. Обновлён npm lockfile. CSS размещает баннеры над flex-layout, добавляет оформление списка участников и присутствия.

### 0.4. Выполненные изменения Rust и стенда

| Находка / область | Изменение | Подтверждение / предел проверки |
|---|---|---|
| AUD-030 | Выход старой сессии сохраняет индекс новой; выход последней по времени восстанавливает индекс оставшейся. Offline не рассылается, пока есть другой вход | `finch-server/src/session.rs`, `quic.rs`; два real-QUIC unit-теста прошли |
| AUD-038 | Решение о первом admin и INSERT объединены в одну SQLite-операцию с `RETURNING role` | `api/auth.rs`; RED: 4 admin из 8 регистраций, GREEN: ровно 1 |
| AUD-039 | REST и QUIC проверяют существование/срок серверной сессии; username/role читаются из актуальной БД. Ошибки записи sessions больше не игнорируются | `verify_active_jwt` в `api/auth.rs`, вызов в `quic.rs` |
| AUD-039, logout | Добавлен `POST /api/auth/logout`, удаляется текущий jti, закрываются QUIC-соединения с этим токеном. У `ClientSession` появился `token_id`, у `AuthUser` — `session_id`. Tauri и browser logout вызывают удалённый отзыв; локальная очистка выполняется и при его ошибке | `auth_revocation.rs` прошёл: прежний токен отвергнут REST/QUIC, второй независимый вход работает. Нового UI бана не добавлено |
| AUD-034 | Снимок онлайн-пользователей читает display_name/avatar из БД и исключает повторение профиля из-за нескольких сессий | новый `finch-server/src/api/state.rs`; тест display name прошёл |
| Снимок состояния | Общий builder состояния для QUIC-auth и авторизованного `GET /api/state`; guards DashMap освобождаются до SQL. Добавлены `ApiClient::server_state` и Tauri `get_session_snapshot` с peer ID, локальными mute/deafen, текущей voice-комнатой и pending TOFU; зарегистрирована команда в `app/src/lib.rs` | `finch-testbed/tests/state.rs`: оба теста прошли до HTTPS-перехода |
| AUD-031 | Повторный запуск закрывает прежний QUIC conn, очищает control sender/last_connect/voice и останавливает аудио. Поколение передаётся в обработчики и reconnect, устаревшее соединение не должно реактивироваться. Состояние conn/control устанавливается до события authenticated. После reconnect отправляется mute/deafen | `app/src/connection.rs`; regression `aud031_replacing_session_closes_previous_quic_connection`: RED timeout, затем GREEN |
| AUD-040 | Вложения истории загружаются одним SELECT для ID выбранной страницы, без lookahead; пустая страница не запрашивает вложения; порядок вложений сохранён | `api/messages.rs`, новый `history_batch_test.rs`; реальный SQL trace RED: 3 SELECT, GREEN: 1 SELECT. Последний объединённый прогон с `api_test` отменён в очереди сборки |
| Ревью #6 | Для HTTP/QUIC graceful drain и endpoint idle введён общий deadline 5 секунд; зависшие задачи abort/join | `launch.rs`; добавлен `shutdown_bounds_a_stalled_graceful_drain`. Его RED (timeout 6 сек.) подтверждён, отдельный GREEN после исправления **не зафиксирован** |
| Ревью #7 | Тест внезапного отключения ждёт именно Offline нужного пользователя | `tests/resilience.rs` |
| Ревью #8 | Сеть измеряется по чирпам в начале/середине/конце (0,2/2,2/4,2 сек.); отсутствие или задержка любого probe >200 мс проваливает тест. Сохранены loss/buffer/gap пороги | `tests/network_impairment.rs`; три регрессии детектора, совмещённый прогон network/resilience: 11 passed; измерено 96,44/116,44/136,44 мс, буфер 6, нулевых кадров подряд 0 |
| Ревью #10 | UDP receive восстанавливается после временных ошибок, включая WSAECONNRESET, с backoff 10–250 мс; fatal ошибки видимы; отложенные отправки принадлежат JoinSet и отменяются при drop | `src/netem.rs`, новый `tests/netem_resilience.rs`; RED после рестарта upstream, GREEN с реальными Windows 10054 и обменом до/после сбоя; прежний proxy_drops_and_delays тоже прошёл |

Старые QUIC/load fixtures теперь создают реальные users/sessions вместе с JWT, чтобы не обходить новую проверку авторизации. У `finch-testbed` добавлена dev-зависимость `serde_json`, у `app` — dev-зависимость `finch-testbed`. Новые интеграционные тесты также находятся в `auth_revocation.rs` и `state.rs`. Полной повторной проверки старых fixtures после всех изменений нет.

### 0.5. AEC3: что реально есть в дереве

- В `finch-audio/Cargo.toml` добавлен pinned `sonora = "=0.2.0"`, `default-features = false`; Cargo.lock обновлён.
- `aec.rs` заменён адаптером AEC3: внешний frame 960/48 кГц сохраняется, обработка разбивается на 480/10 мс; reference собирается последовательно, входные значения sanitise/clamp, состояние адаптивного фильтра сохраняется.
- Есть `set_enabled`, `reset`, `push_reference`, `push_reference_sample`, реальный `set_stream_delay_ms(i32) -> Result<(), sonora::Error>`; удалены неиспользуемые scalar API `set_adaptation_rate`, `set_max_coupling`, `filter_gain`, не имеющие корректного аналога в AEC3.
- Измерение исходного фильтра: **ERLE 0,8 дБ** (не проходит). Первоначальный Sonora-вариант: **77,8 дБ**, near speech −23,3 dBFS, double-talk −25,8 dBFS (потеря 2,5 дБ, проходит порог 6 дБ).
- После этого изменялись clock при silent preroll, reset/delay и unit-tests; основной quality test на последнем коде не повторён. Последние unit-tests: **6/7**, падает `test_aec_double_talk_preserves_speech` даже после добавления convergence. Не ослаблять порог ради зелёного теста без выяснения причины.
- В `new`/`reset` два production `expect()` нарушают правила проекта; их нужно устранить.
- **`app/src/audio_engine.rs` не изменён:** эталон всё ещё подаётся до фактического вывода, callback reference queue/timestamps/overflow reset не подключены. Задержка устройства ещё не передаётся. AUD-021 нельзя закрывать по одному промежуточному headless-замеру.

Проверенные первичные источники: [Sonora API](https://docs.rs/sonora/0.2.0/sonora/struct.AudioProcessing.html), [репозиторий/Windows CI](https://github.com/dignifiedquire/sonora). Нативные `webrtc-audio-processing` bindings не выбраны из-за непроверенного Windows/MSVC build path.

### 0.6. VAD: результаты исследования и пределы

- В `voice_quality.rs` тест `silence_and_noise_do_not_light_speaking` дополнен настоящей `speech_1`, разделением шумовых/речевых пакетов по timestamp и обязательным включением индикатора/доставкой речевых пакетов. `#[ignore = "AUD-050"]` сохранён.
- Последний RED: **2 шумовых speaking transition, 100 шумовых пакетов**; positive control работает: `speech_lit=true`, 139 речевых пакетов. Отсутствие всего трафика больше не даёт ложный успех.
- Проверены корректность 48 кГц, последовательных 480-sample blocks, масштаба RNNoise signed PCM и нормализации pink noise до −35 dBFS. Ошибки формата/масштаба не обнаружено.
- RNNoise даёт высокую вероятность речи на pink noise (выше RMS floor ≥0,9756); простое включение probability ≥0,5 не исправляет баг. Порог около 0,999 теряет речь/тестовые сигналы, не принят.
- Исследован `earshot = "=1.2.2"`, dependency добавлена, **но production-интеграции нет** (`capture.rs`, `vad.rs`, `noise.rs` не изменены). При документированном пороге 0,5 после RNNoise в scratch-пробе — 0 pink positives, сохранены три speech fixtures и речь с pink noise при SNR 5 дБ.
- Ограничение: white noise всё ещё даёт 25 positive frames. AND с RNNoise 0,5 оставляет 20 и полностью отсекает чирп из-за несовпадения временных окон. Этот AND не принят как исправление.
- Earshot замечает 4/5 чирповых кадров с onset +20 мс; интеграционный порог latency ≤120 мс с ним ещё не проверен. Предложены default Speech mode и явный Energy mode лишь для синтетических транспортных измерений; API `VadMode` ещё не добавлен.
- Scratch-проекты находятся только в игнорируемых `target/vad-probe` и `target/vad-probe-build`; это исследование, не часть продукта.

### 0.7. Незавершённый REST TLS

Реально изменён только `finch-client-core/src/api.rs` и добавлен `tests/rest_tls_test.rs`:

- `ApiClient::new` использует обычную CA-проверку, принимает только HTTPS origin, без userinfo/query/fragment/path.
- Удалён `danger_accept_invalid_certs(true)`; `.https_only(true)`, redirects disabled, timeout 30 сек.
- `ApiClient::with_tofu(url, Arc<DashMap<...>>)` использует существующий `TofuVerifier` с обязательным известным pin и проверкой TLS-подписей; ошибки unknown/mismatch возвращаются типизированно.
- Добавлен `verify_trust()` — запрос без пароля/JWT; запросы проходят через общий `send` для обработки trust issue.
- Тесты: запрет plaintext до учётных данных; unknown pin до регистрации, сохранение доверия, restart и смена сертификата. Plaintext regression воспроизведён **до** исправления. **После реализации клиента новые тесты/компиляция не выполнены**.
- Серверный HTTPS listener, общий cert с QUIC, `RunningServer.certificate_fingerprint`, `TestServer.api()`, URL миграция, Tauri trust-before-password пока лишь спроектированы. Никаких этих серверных/desktop-изменений в дереве нет. Сервер всё ещё HTTP.

### 0.8. Инструменты, Git и сборка

- Создана ветка `codex/handoff-bugfixes`. Слияния, push и новых коммитов не было; исходная `audit-and-testbed` не обнаружена среди локальных веток.
- `app/frontend/dist` полностью исключён из Git index (`git rm --cached`); файлы сборки остаются на диске. В Git staged удаления прежних HTML/CSS/logo. JS и остальные generated файлы больше не предполагается хранить частично.
- `.gitignore` дополнен `.npm-cache/` и `.toolchains/`.
- Установлены npm-зависимости frontend, Vitest, happy-dom; обновлён package-lock. Rust stable 1.99.0, Cargo, rustfmt и Clippy установлены локально в `.toolchains`, глобальный PATH не менялся.
- Tauri-сборка затронула `app/gen/schemas/desktop-schema.json` и `windows-schema.json`; при проверке смыслового diff не было, Git сообщал о переводах строк. Не смешивать generated-шум с функциональным исправлением.
- `PROGRESS.md`, `docs/audit/2026-10-audit-report.md`, `BASELINE.md` и ручной checklist пока не обновлены.

Пример окружения PowerShell (из корня проекта):

```powershell
$env:CARGO_HOME = Join-Path $PWD '.toolchains\cargo'
$env:RUSTUP_HOME = Join-Path $PWD '.toolchains\rustup'
$env:PATH = "$env:CARGO_HOME\bin;C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;$env:PATH"
```

Git доступен по пути `C:\Users\maksz\AppData\Local\GitHubDesktop\app-3.6.3\resources\app\git\cmd\git.exe`. В песочнице localhost UDP-тесты зависали, а native CMake rebuild был проблемным; успешные сетевые Rust-прогоны выполнялись вне сетевой песочницы. При необходимости сетевого доступа использовать штатное разрешение, а не обход ограничений.

### 0.9. Проверки: что можно утверждать

| Проверка | Последний подтверждённый результат |
|---|---|
| `npm test` | 27 passed |
| `npm run build` | passed, включая `tsc --noEmit` |
| `cargo check --workspace --all-targets --locked` | passed **до** последних Sonora/Earshot и незавершённых HTTPS-изменений; это не проверка итогового дерева |
| Серверный all-targets прогон после AUD-030/038 | passed на том этапе; позднейшие изменения требуют повторения |
| Testbed `auth_revocation` + `state` | 3 passed до HTTPS-перехода |
| `history_batch_test` | отдельный GREEN; последующий combined прогон отменён до запуска тестов |
| Network impairment + resilience | 11 passed на этапе до дальнейших аудио/HTTPS изменений |
| UDP recovery + прежний proxy test | оба targeted GREEN |
| App `aud031` | targeted GREEN после RED |
| AEC quality | промежуточный GREEN ERLE 77,8 дБ, нужен повтор на последнем коде |
| AEC unit | **6 passed / 1 failed**, double-talk |
| Усиленный ignored VAD test | **RED**, positive speech control проходит |
| REST TLS tests после client implementation | не запускались, server TLS отсутствует |
| Полный `cargo test --workspace` / Clippy итогового дерева | не выполнены |
| Ручные голос, устройства, настоящий screen share, Tauri TOFU | не выполнены |

### 0.10. Что продолжать

1. Восстановить согласованную работу client/server: закончить HTTPS + TOFU до пароля и миграцию тестовых API/URL. Учесть уже добавленные logout и snapshot.
2. Устранить production `expect()` и разобраться с падающим AEC double-talk; перепроверить исходные метрики. Подключить фактический playback reference в audio engine, затем AUD-024 (выбор/восстановление устройств).
3. Довести VAD на основании измерений, интегрировать выбранный discriminator, сохранить positive speech/latency проверки и явно проверить white noise.
4. Закрыть оставшиеся AUD-018/035 (экран целиком и uni-stream limit/concurrency), AUD-032 (CSP), AUD-036 (корреляция async server rejection). Код для экрана, CSP и request IDs **ещё не менялся**; миграции БД/новых вариантов протокола для request IDs ещё нет.
5. Перепроверить shutdown deadline GREEN, весь workspace и frontend, затем ревью и доступные ручные сценарии. Намеренная runtime-защита `server_liveness.rs` пока не менялась.
6. Обновить отчёт/PROGRESS/BASELINE и статусы каждой находки только по фактам. После общего зелёного прогона разобрать staged/unstaged/untracked и оформить коммиты с ID. Не считать все баги закрытыми по этому handoff.

---

## Архив исходной передачи

Следующие разделы сохранены для контекста предыдущего этапа. Их статусы и планы сверять с актуальным разделом 0 выше.

## 1. Что за продукт и для кого

FinchSpeak — self-hosted аналог Discord **для своей компании или друзей: один сервер, до ~50 человек, клиенты только на Windows**. Гильдии (много серверов) и горизонтальное масштабирование не нужны.

Решения владельца, которые уже приняты и не обсуждаются заново:

- **E2EE не делаем.** Шифрование — TLS 1.3 в QUIC между клиентом и своим сервером. Примитивы ChaCha20 в `finch-protocol` остаются на будущее (AUD-001, AUD-029).
- **TLS для REST** — в подпроекте 6 (AUD-003).
- **Демонстрация экрана** — в подпроекте 2 (AUD-018).
- **Шумоподавление:** порог «+10 дБ SNR» действует только для стационарного шума; фоновые разговоры только измеряются (AUD-049).

## 2. Как устроен код (кратко)

| Крейт | Что делает |
|---|---|
| `crates/finch-protocol` | Протокол: 20-байтный `MediaHeader`, сообщения управляющего потока (MessagePack), кодек, примитивы E2EE |
| `crates/finch-server` | Сервер: QUIC SFU + REST (axum) + SQLite. Точка входа для тестов — `finch_server::launch::launch(config)` |
| `crates/finch-audio` | Аудио без ввода-вывода: `VoiceCapture` (AEC → шумодав → VAD → Opus → пакет), `AudioMixer`, jitter buffer, ресемплеры |
| `crates/finch-screen` | Захват экрана, H.264, фрагментатор. **Приложением пока не используется** (AUD-018) |
| `crates/finch-client-core` | Клиент без UI: QUIC-сессия (события в канал), TOFU, REST-клиент, переподключение |
| `crates/finch-testbed` | Тестовый стенд: настоящий сервер + headless-клиенты, голосовые метрики, UDP-прокси с потерями |
| `app` | Tauri v2: Rust-обёртка над `finch-client-core` и `finch-audio` + фронтенд SolidJS в `app/frontend` |

Правила проекта (требования 1–11 в [PROGRESS.md](../PROGRESS.md)), самые важные:

- нет `unwrap()` / `expect()` / `todo!()` в production-коде сети, сервера и аудио;
- `finch-server` не зависит от `finch-audio`, `finch-screen`, `finch-client-core`;
- в аудио-колбэках `cpal` нет мьютексов и аллокаций;
- перед коммитом проходят `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `npm run build` в `app/frontend`.

Как работали в фазе 3 и как продолжать: **один баг — один коммит**, сначала падающий тест (в `finch-testbed` или юнит-тест), потом исправление, в сообщении коммита ID находки (`AUD-NNN`).

## 3. Как запустить тесты

```bash
cargo test --workspace
```

```bash
cargo test -p finch-testbed -- --include-ignored --nocapture
```

Второй вариант прогоняет и проигнорированные тесты (они воспроизводят открытые баги) и печатает строки `BASELINE` с голосовыми метриками. Текущие значения — в [crates/finch-testbed/BASELINE.md](../crates/finch-testbed/BASELINE.md).

На момент передачи: **140 тестов зелёные, 2 проигнорированы** (`AUD-021`, `AUD-050`).

---

## 4. Что уже исправлено (фаза 3)

| ID | Что было | Чем закрыто (тест) |
|---|---|---|
| AUD-001 | В README/PROGRESS заявлен E2EE, которого нет | Заявления убраны (документация) |
| AUD-002 | TOFU не проверял подпись TLS — возможен MITM | `tofu_mitm_test::certificate_without_matching_key_is_rejected` |
| AUD-004 | Одно неудачное рукопожатие останавливало QUIC-сервер | `resilience::server_survives_failed_handshake` |
| AUD-005 | Сертификат сервера новый на каждом старте; отпечатки не сохранялись; модалка TOFU не вызывалась | `quic::tests::certificate_is_persisted_across_restarts`, `tofu_store_test::*` |
| AUD-006 | Ссылка DashMap через `.await` → дедлок сервера | `server_liveness::server_survives_client_that_never_reads` |
| AUD-007 | Один медленный клиент тормозил рассылку всем | `server_liveness::stalled_client_does_not_block_chat_for_others` |
| AUD-008 | Обрыв клиента не рассылал `UserLeftVoice` | `resilience::abrupt_disconnect_cleans_presence` |
| AUD-009 | Новые каналы не регистрировались в SFU и не рассылались | `channels::created_voice_channel_is_announced_and_joinable` (новое сообщение `ServerMessage::ChannelCreated`) |
| AUD-011 | Порядок истории внутри секунды, пагинация теряла сообщения | `chat::history_persisted_in_order`, `chat::history_pages_by_message_id` (курсор `before_id`) |
| AUD-012 | Файлы > 2 МБ не загружались | `files::upload_up_to_configured_limit_succeeds` |
| AUD-014 | Файлы скачивались без авторизации | `files::download_requires_auth` |
| AUD-015 | Можно было прикрепить чужой файл; вложения не приходили в реальном времени | `files::attachments_are_delivered_and_cannot_be_stolen` |
| AUD-016 | Сообщения без проверки канала и длины, ошибки молча терялись | `chat::invalid_messages_are_rejected_with_error` |
| AUD-017 | Подписка на чужую демонстрацию экрана без проверки комнаты | `screen::outsider_cannot_subscribe_to_screen` |
| AUD-022 | Jitter buffer: поздние пакеты копились, очередь росла до 68 пакетов | `jitter::tests::*`, `network_impairment::voice_under_loss_and_jitter` |
| AUD-023 | Конкуренция за микшер вставляла 20 мс тишины | `audio_engine::tests::test_mixer_contention_does_not_insert_silence` |
| AUD-025 | Не было автопереподключения | `resilience::client_reconnects_automatically_when_server_returns`, `reconnect_stops_on_rejected_token` |
| AUD-026 | Зашитый JWT-секрет по умолчанию | `config::tests::test_*secret*` (секрет генерируется в `data/jwt_secret`) |
| AUD-028 | QUIC-порт угадывался из HTTP-порта | `auth::login_reports_quic_port` (порт приходит в ответе логина) |
| AUD-047 | Отказ авторизации приходил как «connection lost» | `session_test::invalid_token_rejected_on_quic` |
| AUD-048 | Микрофон не на 48 кГц терял звук (16 кГц — 2/3) | `resampler::tests::test_input_resampler_preserves_duration` |
| AUD-051 | Наушники 16 кГц играли 1/3 звука, 96 кГц — тишина | `resampler::tests::test_output_resampler_preserves_duration` |
| AUD-052 | Испорченный кадр управляющего потока молча «глушил» клиента | `control_stream_test::corrupt_control_frame_ends_the_session_visibly` |

Снято: **AUD-013** не подтвердилось (кириллические имена файлов скачиваются, тест оставлен как защита); **AUD-029** снят вместе с E2EE.

---

## 5. Нерешённые ошибки

### 5.1. Осталось в подпроекте 1 — фронтенд (делать первым)

Все — в `app/frontend/src`. Автотестов фронтенда пока нет. **Первый шаг:** подключить Vitest (`npm i -D vitest`, скрипт `"test": "vitest run"`), вынести логику из компонентов в функции хранилищ (`stores/*.ts`) и покрыть их тестами.

| ID | Ур. | Проблема | Где | Что сделать | Размер |
|---|---|---|---|---|---|
| AUD-053 | К | Отправка сообщения падает: `currentUser` не импортирован | `stores/chatStore.ts:43` | Импорт из `authStore`; тест `sendMessage` | S |
| AUD-055 | К | Второе входящее сообщение роняет обработчик: живые сообщения приходят с `message_id`, история — с `id`, код делает `m.id.startsWith` | `stores/chatStore.ts:63-69`, `types.ts` | Привести всё к одному виду (`id`) на входе, `handleIncomingMessage` — тест | S |
| AUD-010 | В | История чата показывается перевёрнутой: сервер уже отдаёт хронологически, клиент переворачивает ещё раз | `stores/chatStore.ts:30` | Убрать `reverse()` | S |
| AUD-054 | В | Создание канала падает: `api`, `loadChannels` не импортированы | `components/Sidebar.tsx:45-46,76-77` | Импорты | S |
| AUD-019 | В | UI не слушает события: кто уже в голосе (`authenticated.initial_state.voice_states`), mute/deafen других (`user-voice-state-changed`), `presence-update`, обрыв связи (`connection-lost`, `reconnecting`, `reconnected`, `reconnect-failed` — Rust их уже шлёт), `server-error` | `App.tsx`, `services/api.ts`, `stores/voiceStore.ts` | Подписки + баннер «Нет соединения / Переподключение…»; ошибки сервера (например 400 на слишком длинное сообщение) показать пользователю | M |
| AUD-020 | В | Участники чужих голосовых каналов попадают в вашу панель | `App.tsx:39-45` | Хранить участников по `channel_id`, показывать активный канал (и в сайдбаре под каналом, как в Discord) | S |
| AUD-056 | М | Сборка не проверяет типы; `tsc --noEmit` даёт 10 ошибок | `package.json` | `"build": "tsc --noEmit && vite build"`, исправить все ошибки (3 из них — AUD-053/054/055) | S |

Связанное: `app/frontend/dist/index.html` закоммичен, а JS-бандл — нет (артефакт сборки отслеживается частично). Решить: либо не хранить `dist/` в git, либо хранить целиком.

### 5.2. Открытые баги, перенесённые в следующие подпроекты

| ID | Ур. | Проблема | Куда | Тест / как воспроизвести |
|---|---|---|---|---|
| AUD-003 | К | REST по открытому HTTP: пароли и JWT в открытом виде | подпроект 6 | — (решено: TLS для REST тем же самоподписанным сертификатом + TOFU, или REST поверх QUIC) |
| AUD-018 | К | Демонстрация экрана не работает целиком (нет запуска захвата, `ScreenViewer` не подключён, `ScreenAudio` игнорируется, нет автозапроса ключевого кадра, ключевые кадры > 1 МБ отбрасываются сервером — AUD-035) | подпроект 2 | протокольная часть покрыта `finch-testbed/tests/screen.rs` |
| AUD-021 | В | Эхоподавление не работает с реальным эхом: ERLE 0,8 дБ при пороге 20 (самописный фильтр с окном 1,3 мс вместо AEC3) | подпроект 2 | `voice_quality::echo_cancelled_after_convergence` (`#[ignore]`) |
| AUD-050 | В | VAD по громкости: вентилятор −35 dBFS зажигает индикатор речи и уходит в сеть | подпроект 2 | `voice_quality::silence_and_noise_do_not_light_speaking` (`#[ignore]`) |
| AUD-024 | П | Нет выбора аудиоустройства; отключение гарнитуры молча убивает звук | подпроект 2 | ручной чек-лист |
| AUD-033 | М | Снятие deafen оставляет микрофон выключенным | подпроект 2 | — |
| AUD-034 | М | В списке онлайн вместо отображаемого имени — логин, аватар `None` | подпроект 3 | — |
| AUD-036 | М | Оптимистичное сообщение не откатывается при ошибке отправки | подпроект 4 | — |
| AUD-037 | М | Кнопки создания канала видны не-админам (сервер отвечает 403) | подпроект 5 | — |
| AUD-039 | М | Таблица `sessions` не проверяется: logout и бан не отзывают токен | подпроект 5 | — |
| AUD-032 | М | В Tauri выключен CSP (`"csp": null`) | подпроект 6 | — |

### 5.3. Мелочи без подпроекта (взять по пути)

| ID / источник | Проблема | Где |
|---|---|---|
| AUD-030 | Второй вход того же пользователя затирает `user_to_peer`; выход первого удаляет запись второго | `finch-server/src/session.rs` |
| AUD-031 | Повторный логин не закрывает старую QUIC-сессию (переподключение теперь отменяется, но старое соединение не закрывается явно) | `app/src/commands/auth.rs` |
| AUD-035 | Лимит ключевого кадра: 1 МБ на сервере против 10 МБ на клиенте; uni-стримы читаются по одному | `finch-server/src/quic.rs` — делать вместе с AUD-018 |
| AUD-038 | Гонка «первый зарегистрированный — админ» | `finch-server/src/api/auth.rs` |
| AUD-040 | История: отдельный запрос вложений на каждое сообщение (N+1) | `finch-server/src/api/messages.rs` |
| ревью #6 | Ctrl+C ждёт graceful-drain без таймаута | `finch-server/src/launch.rs::shutdown` |
| ревью #7 | Тест `server_responsive_after_abrupt_disconnect` принимает любой `PresenceUpdate`, а не только `Offline` | `finch-testbed/tests/resilience.rs` |
| ревью #8 | Задержка в сценарии плохой сети меряется только по первому чирпу | `finch-testbed/tests/network_impairment.rs` |
| ревью #9 | Тест VAD на шуме пройдёт впустую при отсутствии трафика — при исправлении AUD-050 добавить в конце фразу и проверить, что индикатор загорается | `finch-testbed/tests/voice_quality.rs` |
| ревью #10 | UDP-прокси стенда останавливается на первой ошибке приёма (на Windows — `WSAECONNRESET`) | `finch-testbed/src/netem.rs` |
| фаза 3 | Тест-сервер в `server_liveness.rs` оставляет поток с runtime до конца процесса (намеренно, чтобы дедлок не вешал тест) | `finch-testbed/tests/server_liveness.rs` |

### 5.4. Что не проверено руками

- **[Ручной чек-лист голоса](audit/manual-voice-checklist.md) не пройден.** Это обязательная проверка рефакторинга аудио (раздел 0) и находок, видимых только в GUI (раздел 5). Перед проверкой записать частоты устройств.
- Новые потоки после фазы 3 без автотестов (Tauri-обвязка): окно подтверждения сертификата (`tofu-required` → `TofuModal`), переподключение с повторным входом в голосовой канал, появление нового канала в списке (`channel-created`).

---

## 6. План дальше

### 6.1. Закрыть подпроект 1

1. Фронтенд: Vitest + AUD-053, 055, 010, 054, 019, 020, 056 (таблица 5.1).
2. Пройти ручной чек-лист, записать результат в отчёт.
3. Обновить [PROGRESS.md](../PROGRESS.md): реальное число тестов, метрики из `BASELINE.md`, этапы 3 и 5 — как есть (экран не работает, E2EE не делается).
4. В отчёте проставить статус у каждой находки «Критично» и «Важно»: исправлено (коммит) / перенесено (подпроект).
5. Финальное ревью ветки, затем слияние `audit-and-testbed` в `main`.

### 6.2. Новые фичи — в планах, не согласованы

Ниже — черновой состав подпроектов 2–6. **Ничего из этого не согласовано с владельцем.** Каждый подпроект начинается с обсуждения состава, затем спек → план → реализация, как подпроект 1. Ориентир — «как в Discord», но для 50 своих людей: без лишней сложности.

#### Подпроект 2 — Голос и экран (следующий по приоритету)

| Фича | Зачем | Заметки |
|---|---|---|
| Выбор микрофона и наушников, горячая смена, восстановление после отключения гарнитуры | AUD-024 | `cpal` уже перечисляет устройства (`get_audio_devices`), UI нет |
| Тест-петля «слышу себя» и индикатор уровня микрофона | AUD-042 | |
| Push-to-talk с глобальной горячей клавишей | Discord-стандарт | Tauri global-shortcut |
| Ползунок чувствительности VAD + VAD по вероятности речи RNNoise | AUD-050 | `DenoiseState::process_frame` возвращает вероятность речи |
| Нормальное эхоподавление (AEC3: `webrtc-audio-processing` или `aec3-rs`) | AUD-021 | Референс подавать в момент фактического воспроизведения |
| Громкость каждого участника (сохранять между сессиями) | | `set_speaker_volume` есть, не сохраняется |
| Восстановление mute после снятия deafen | AUD-033 | |
| **Демонстрация экрана целиком** | AUD-018, AUD-035 | Команды старта и стопа, выбор экрана или окна, `ScreenViewer` в `VoicePanel`, звук экрана, автозапрос ключевого кадра; поднять лимит IDR на сервере |
| Индикатор качества связи (ping, потери) | | RTT уже приходит (`ping-rtt`), потери есть в `AudioMixer::loss_rate` |

#### Подпроект 3 — Профиль и настройки

| Фича | Заметки |
|---|---|
| Аватарки: загрузка, обрезка, показ в чате, голосе и списке онлайн | В БД есть `users.avatar_path`, API нет; AUD-034 |
| Отображаемое имя и смена пароля | |
| Статус: онлайн / не на месте / не беспокоить / невидимка | `users.status` в БД есть |
| Экран настроек: аккаунт, голос и видео (из подпроекта 2), внешний вид (светлая и тёмная тема), горячие клавиши, уведомления | |
| Сохранение настроек клиента локально (сервер, логин, устройства) | |

#### Подпроект 4 — Чат как в Discord

| Фича | Заметки |
|---|---|
| Редактирование и удаление своих сообщений | `messages.edited_at` в БД есть |
| Ответы на сообщения | `messages.reply_to_id` в БД есть |
| Реакции эмодзи | Таблица `message_reactions` есть, API и UI нет |
| Упоминания `@user` с подсветкой и уведомлением | |
| Индикатор «печатает…» | |
| Непрочитанные: счётчик на канале, разделитель «новые сообщения» | |
| Загрузка файлов из UI (кнопка и drag-and-drop), превью картинок, ссылки на скачивание | AUD-041; сервер готов |
| Подгрузка старой истории при прокрутке | Курсор `before_id` уже есть (`ApiClient::fetch_messages_page`) |
| Откат оптимистичного сообщения при ошибке | AUD-036 |
| Markdown-разметка в сообщениях, превью ссылок | По желанию |

#### Подпроект 5 — Модерация и права

| Фича | Заметки |
|---|---|
| Регистрация по приглашениям (ссылка или код) или паролю сервера | AUD-027 — сейчас кто угодно с адресом может зарегистрироваться |
| Роли: админ / модератор / участник; права на создание каналов, кик, бан | AUD-037 |
| Каналы: переименование, удаление, порядок (drag-and-drop), тема | |
| Кик из голосового канала, server mute | |
| Бан с отзывом токенов | AUD-039: проверять таблицу `sessions` при авторизации |
| Журнал действий админа | По желанию |

#### Подпроект 6 — Продакшен

| Фича | Заметки |
|---|---|
| TLS для REST | AUD-003, решение владельца |
| Установщик Windows (MSI или NSIS через Tauri bundler) и автообновление (Tauri updater, подписанные релизы) | |
| CSP в Tauri | AUD-032 |
| Бэкапы SQLite (`VACUUM INTO` по расписанию) и восстановление | |
| Логи сервера в файл с ротацией; простая страница здоровья (`/health`) | |
| Docker: том `data/` хранит сертификат, JWT-секрет, БД и вложения — описать в README | Конфиг `deploy/finch-server.toml` уже обновлён |
| Сборка релиза в CI (GitHub Actions): тесты, clippy, сборка клиента | |

---

## 7. Полезные файлы

- [docs/audit/2026-10-audit-report.md](audit/2026-10-audit-report.md) — все находки с `файл:строка`, шагами воспроизведения и предложенными исправлениями.
- [docs/audit/manual-voice-checklist.md](audit/manual-voice-checklist.md) — ручная проверка голоса.
- [crates/finch-testbed/BASELINE.md](../crates/finch-testbed/BASELINE.md) — измеренные голосовые метрики.
- [docs/superpowers/specs/2026-10-15-audit-and-testbed-design.md](superpowers/specs/2026-10-15-audit-and-testbed-design.md) — спек подпроекта 1, пороги метрик.
- [docs/superpowers/plans/2026-10-15-audit-and-testbed.md](superpowers/plans/2026-10-15-audit-and-testbed.md) — план подпроекта 1.
- `git log main..audit-and-testbed` — каждое исправление отдельным коммитом с ID находки.
