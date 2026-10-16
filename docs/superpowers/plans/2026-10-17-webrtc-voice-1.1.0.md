# WebRTC Voice 1.1.0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Владелец выбрал реализацию основным исполнителем в этом чате и одно независимое итоговое ревью; реализацию по задачам не делегировать.

**Goal:** Полностью внедрить оригинальные WebRTC AEC3/AGC2/limiter вместе с существующим RNNoise и выпустить проверенную стабильную Windows-версию v1.1.0.

**Architecture:** Состояния original C++ APM скрыты за узким safe Rust adapter, отдельно AEC-only и AGC-only, между ними существующий RNNoise. Capture управляет pre-AGC VAD/метром, приватностью и Opus; UI сохраняет одну настройку. Build, CI, установка и опубликованные artifacts проверяются как часть результата.

**Tech Stack:** Rust, native WebRTC C++, RNNoise через nnnoiseless, Earshot, Opus, cpal, Tauri 2, SolidJS/Vitest, PowerShell, GitHub Actions Windows x64.

**Spec:** `../specs/2026-10-17-webrtc-voice-1.1.0-design.md`. Этот план заменяет невыпущенную custom-DSP часть `2026-10-17-voice-enhancement.md`. Статус: представлен для согласования, выполнение по нему ещё не начато.

## Global Constraints

- Windows x64 MSVC; mono 48 kHz, 960 samples/20 ms, native blocks 480/10 ms; Opus 64 kbit/s и MediaHeader без изменений.
- Только original C++ AEC3/AGC2/limiter; удалить Sonora и собственный VoiceDynamics из production, сохранить готовый RNNoise/nnnoiseless/Earshot.
- RNNoise-модель встроенная и прежняя; один NS и один gain controller; без новых системных сервисов/плагинов/виртуальных устройств/загрузок при работе приложения.
- Один зафиксированный native build route после Task 1; source/toolchain hashes и лицензии зафиксированы. Никаких floating HEAD, случайных prebuilt DLL и правок алгоритмов upstream.
- Все DSP в audio worker; cpal callbacks только ring buffers. Новые adapters после warm-up не аллоцируют, не блокируют, не делают I/O; не добавляют frame queue.
- AGC2: initial 0 dB, adaptive max +12 dB, rate 6 dB/s, noise -50 dBFS, headroom 6 dB, analog controller off; tuning только в диапазонах spec. 0 trim = silence. Enabled steady-state peak ≤0.891251+1e-6; transition ceiling к концу fade20ms; off peak ≤1.
- AGC не открывает пользовательский threshold/PTT; meter pre-AGC, mic test/monitor final PCM. Приватный pre-roll очищается. Pre-roll80ms/hangover100ms сохранены.
- Старые AEC/NS/VAD/network gates не ослаблять. Новые критерии готового AGC2 из spec явно заменяют только невыпущенные требования к самописному compressor.
- Стабильный v1.1.0, совместимость с сервером1.0.1, signed updater для всех клиентов; не перемещать опубликованные теги. Секрет/ключ не выводить и не коммитить.
- Пользовательские работающие серверы, базы/сертификаты и установки не изменять тестами; свободные порты, отдельные профили, остановка только своих PID.

## Review Focus

- MSVC/C++ ABI/CRT, cache-miss и кириллический путь: установленный клиент запускается без окружения сборки — Tasks 1,2,9.
- Вставка RNNoise между AEC/AGC, normalized API и ручные 300%/0%: нет двойной обработки или clipping до limiter — Tasks 2–4.
- Mute/PTT/mic test/reconnect: private frames и старые частичные блоки не отправляются после открытия — Tasks 4,7.
- Быстрые изменения настроек, explicit false и native failures: UI/storage не сообщают ложный успех, raw mic не уходит в сеть — Tasks 2,4,5.
- Stationary noise versus competing speech, loud jumps и onset: шумоподавление не маскируется gain, слова не пропадают, latency probes проверяются каждый — Tasks 6,7.

## Workspace и ответственность файлов

Использовать текущий managed worktree `C:\Users\Михаил\.codex\worktrees\voice-enhancement\FinchSpeak`, ветку `codex/voice-enhancement`, continuation base cf4ac96. Два изменённых generated schema JSON после прежних тестов сначала проверить на semantic diff и не включать случайно. Новые commits поверх существующих, без reset/rebase опубликованного кода. Коммит текущей пары spec/plan содержит только документы.

- `native/webrtc/`: lock/provenance, minimal probe, выбранный build route и при необходимости `bridge.h`, `bridge.cc`, `BUILD.gn`; никаких собственных DSP-алгоритмов.
- `scripts/prepare-webrtc.ps1`, `scripts/test-client-runtime.ps1`, `.github/workflows/{ci,release}.yml`: provisioning/cache, native dependency и runtime/package gates.
- `crates/finch-audio/src/webrtc.rs`, `aec.rs`, `capture.rs`, `lib.rs`, Cargo files: safe adapter, замена backend, gain settings/errors; `dynamics.rs` удалить после замены callers.
- `app/src/{state,audio_engine}.rs`, `app/src/commands/voice.rs`, frontend voice types/store/API/modal/sessionEvents/tests: persisted toggle, обработка ошибок и worker wiring.
- `crates/finch-testbed/src/{voice,client,metrics}.rs`, tests `voice_enhancement.rs`, `voice_quality.rs`, `voice_compatibility.rs`, existing regression suites: одинаковый production capture в проверках.
- `docs/{NATIVE-AUDIO,THIRD-PARTY-NOTICES,QA-1.1.0,RELEASE-NOTES-1.1.0}.md`, `crates/finch-testbed/BASELINE.md`, README/checklists: воспроизводимость, измерения, ограничения и гайд владельцу.

### Task 1: Доказать нативную Windows-сборку оригинального WebRTC

**Files:** Create `native/webrtc/dependencies.lock.json`, `native/webrtc/probe/`, `scripts/prepare-webrtc.ps1`, `docs/NATIVE-AUDIO.md`; modify CI для native smoke workflow_dispatch/PR до release.
**Interfaces:** script `prepare-webrtc.ps1 -Configuration Debug|Release -CacheDirectory <path> [-Offline]` создаёт build/cache и печатает только пути/публичную provenance; задаёт контракт `FINCH_WEBRTC_ROOT` для следующего task. Успех — exit0 и runnable x64 probe, ошибка — ненулевой exit без fallback-DSP.

- [ ] Написать probe/tests: создать AEC-only и AGC2-only, передать 100 блоков нулей 480f32, получить ровно 480 finite samples; create/drop 100 раз. Запустить без библиотек: ожидаемый RED — отсутствующий native dependency, сохранить лог.
- [ ] Проверить точный tagged release готовой Rust-обёртки и её source submodule, AGC2 API, native MSVC debug/release. Если сборка требует patch build plumbing, оценить его против narrow C ABI; не править DSP и не использовать MinGW library с MSVC Rust.
- [ ] При неподходящей обёртке собрать original C++ через upstream GN/Ninja для x64 MSVC ABI и тонкий C ABI probe. Выбрать стабильную upstream ревизию; записать commit, DEPS revisions, архивные SHA256, Rust/MSVC/SDK/LLVM/GN/Ninja/Python versions и build args в lock/provenance. Из двух путей оставить один.
- [ ] Сделать prepare script идемпотентным: source acquisition с hash validation отдельно от offline build; -Offline с неполным cache явно падает. Не запускать git/tools, скачивающие код, неявно из audio process.
- [ ] Запустить probe Debug/Release локально из пути с пробелом/кириллицей, затем в чистом windows-latest runner без cache и с cache. После зависимости subprocess test запускается с PATH без build directories.
- [ ] Зафиксировать объём adapter, время/cache size, CRT/imports и все лицензии. При невозможности сборки обоими маршрутами остановить именно интеграцию, сообщить конкретный blocker, не выпускать подмену.
- [ ] Commit `build(audio): pin and validate native WebRTC on Windows`.

### Task 2: Safe adapter, владение и ошибки

**Files:** Create `crates/finch-audio/src/webrtc.rs`, native bridge выбранного route; modify `crates/finch-audio/{Cargo.toml,build.rs,src/lib.rs}`, workspace/Cargo.lock; tests module webrtc.
**Interfaces:** `ProcessingError`; `WebRtcAec::new() -> Result<Self, ProcessingError>`, `push_render(&[f32;960])`, `process(&mut [f32;960])`, `reset()`, `set_delay_ms(u32)` — все native-fallible операции возвращают `Result<(), ProcessingError>`. `WebRtcGain::new()`, `set_enabled(bool)`, `set_input_gain(f32)`, `process(&mut [f32;960])`, `reset()` с теми же Result. Native handle private, RAII Drop; objects принадлежат одному worker, не Sync.

- [ ] RED tests exact block count/order (2 последовательных 480), 960 output length, reset=new state, repeated setters do not reconfigure, failure returns Err и не публикует частично обработанный frame; injected native init/process/render failure через test backend.
- [ ] Implement fixed-buffer adapters, C ABI integer status + opaque handle в fallback route; проверять pointers/length до FFI; исключения C++ не пересекают ABI, allocation errors — status, no unwind/UB. Держать unsafe в одном модуле с safety comments.
- [ ] Конфиг AEC-only: AEC3+штатный HPF, NS/AGC off. Gain-only: AEC/NS/HPF off, AGC2 enabled согласно spec. Reverse reference только AEC. Нормализованный PCM на границе, finite validation выхода.
- [ ] Реализовать manual gain split по spec; штатный fixed gain runtime setter для boost, input attenuation для <1. Enabled output -1 dB constant scale; on/off fade20ms без новой очереди. Не реализовывать compressor law, envelope follower или адаптивный gain самостоятельно.
- [ ] GREEN tests с реальной библиотекой: zero/NaN/Inf/extreme input и render, trim0/1/3/8, 200 toggle/reset cycles, destroy exactly once; отдельный тест stereo/invalid block в C bridge возвращает ошибку, если bridge имеет такой вход.
- [ ] Проверить отсутствие новых adapter allocations после warm-up с native/Rust allocator accounting; измерить single-stage/full frame cost release без хрупкого CI wall-clock assert. Проверить что конфигурации меняются только при изменениях, не 50 раз/сек.
- [ ] Commit `feat(audio): wrap original WebRTC AEC3 and AGC2 safely`.

### Task 3: Заменить Sonora AEC и удалить собственную динамику

**Files:** Modify `crates/finch-audio/src/{aec,capture,lib}.rs`, Cargo files; delete `src/dynamics.rs` после переключения; modify AEC tests/callers и testbed helpers.
**Interfaces:** `EchoCanceller::new() -> Result<Self, ProcessingError>`, прежние setters/reference/process/reset становятся fallible где требуется. `VoiceCapture::new(config) -> Result<Self, CaptureError>`; `CaptureError` объединяет `ProcessingError` и `AudioOpusError`. `VoiceCapture::process(..) -> Result<CaptureOutput, CaptureError>`; все callers обновить, никакого unwrap в production worker.

- [ ] RED регрессионные AEC tests: silent-reference/headphones, echo 40ms, изменённая delay, near speech+far speech, reference interrupted; исходные thresholds оставить. Native failure в capture => Err, packets отсутствуют.
- [ ] Заменить backend в aec.rs с сохранением reference feeder и device delay; адаптер не содержит второй обработки NS/AGC. Разделить reset AEC при discontinuity от reset всего source.
- [ ] В capture подключить `AEC3 → RNNoise → pre-AGC analysis → WebRtcGain → pre-roll/Opus`. Не применять старый gain/clamp до AEC; analysis trim не менять реальный сигнал до limiter. Output meter pre-AGC, frame final PCM.
- [ ] Удалить собственные AGC/compressor/limiter module и Sonora зависимости, обновить exports/Cargo.lock. Сохранить либо перенести тесты на externally-visible поведение готовой библиотеки; не оставлять мёртвую альтернативу DSP.
- [ ] GREEN `cargo test -p finch-audio --locked --offline`, `cargo check --workspace --all-targets --locked --offline`; audit dependency graph `cargo tree -p finch-audio` — no sonora, no custom dynamics, один RNNoise.
- [ ] Commit `refactor(audio): use native WebRTC throughout capture`.

### Task 4: Контроль передачи, lifecycle и приватность

**Files:** Modify `capture.rs` tests, `app/src/audio_engine.rs`, `crates/finch-testbed/src/voice.rs`; tests privacy/lifecycle.
**Interfaces:** прежние `set_transmit_gate`, `set_voice_enhancement`, threshold/input setters; результат capture из Task3. `reset_echo_canceller` касается только AEC; source/session recreation сбрасывает всё. `run_voice_link_with_config(sender, receiver, input, tail_frames, config) -> anyhow::Result<VoiceRun>`; старые helpers делегируют ему default-on.

- [ ] RED tests: уникальный private marker в PTT/mute/mic-test кадрах; после reopening нет private timestamps/marker в decoded packets; gate closure очищает и накопленный VAD pre-roll. PCM для local mic test отличается on/off ожидаемо при loud/quiet source даже с закрытым gate.
- [ ] Обработать mute до раннего continue audio worker: clear network pre-roll, partial source blocks/monitor/test queues по переходам, не оставлять звук предыдущего сеанса. Проверить mute/unmute сразу после <80ms quiet window, во время hangover и активной речи.
- [ ] Сохранить normal preroll oldest-first, timestamps/wrapping sequence и FLAG_SILENCE. Проверить quiet-below-threshold never opens при on/off; AGC внутренний VAD не управляет сетевым gate.
- [ ] Worker читает atomics каждый frame, setters idempotent; source recreation reapply settings, no old gain/partial blocks. Reference overflow resets AEC only. Device loss/reconnect не приводит к отправке накопленного private audio.
- [ ] Native init failure сразу сообщает UI; processing error drop frame и clear preroll, 3 consecutive failures останавливают capture и посылают одно `audio-processing-failed` событие. После отказа никакого raw bypass, encoder/queue не используют старый frame.
- [ ] GREEN finch-audio/app tests и targeted privacy e2e с настоящим Opus; Commit `fix(voice): preserve privacy and lifecycle with native processing`.

### Task 5: Полное применение настройки и ошибок в UI

**Files:** Verify/modify `app/src/{state,audio_engine}.rs`, `commands/voice.rs`; frontend `types.ts`, `services/{api,sessionEvents}.ts`, `stores/{voiceStore,notificationStore}.ts`, `components/voice/VoiceSettingsModal.tsx`, related tests.
**Interfaces:** `VoiceSettingsDto.voice_enhancement: bool` serde default=true, AtomicBool/getter; TS property required; `audio-processing-failed` payload `{message:string}` через existing session event subscription показывает notification и сбрасывает speaking/mic-test status.

- [ ] RED Rust serde migration old DTO=>true, explicitfalse roundtrip, atomic snapshot/apply, recreation reads requested mode; wrong numeric inputs bounded. Существующие тесты cf4ac96 переиспользовать.
- [ ] RED Vitest migration missing/corrupt=>true, explicitfalse survives restart, API rejection leaves state/storage, быстрые изменения не позволяют позднему ответу вернуть устаревшее состояние; processing failure виден и очищает speaking/mic-test.
- [ ] Подключить toggle с текстом spec, точный pre-AGC meter hint; сохранить все текущие mic controls. Mic test/Hear myself берут тот же production PCM; никаких отдельных processors/двойного усиления.
- [ ] Сохранить поддержку old saved input_gain/threshold/PTT/key/monitor, key_down не сохранять. При найденной race сериализовать settings writes в существующем store, без отдельной settings-системы.
- [ ] GREEN `cargo test -p finch-app --lib --locked --offline`, frontend `npm test -- --run` и `npm run build`; component tests label/default/click/error/restoredfalse и event unsubscribe. Commit `feat(app): finish native voice enhancement controls`.

### Task 6: Объективное качество и готовый профиль AGC2

**Files:** Create `crates/finch-testbed/tests/voice_enhancement.rs`; modify `src/metrics.rs`, `tests/voice_quality.rs`, `BASELINE.md`; WAVs under `target/voice-enhancement-out` excluded from git.
**Interfaces:** helper Task4, `gain_calibrated_snr_db(clean, processed, common_voiced_windows, lag_samples) -> f32` для диагностики с фиксированными общими окнами, alignment и least-squares gain. Старые `snr_db`/segmental functions не переписывать.

- [ ] RED metric tests: одинаковый SNR при общем gain×0.25/×4 в допуске0.1dB; signal+known noise оценивается корректно; silence/empty inputs дают documented finite/error result; delay alignment фиксируется до сравнения on/off, не подбирается по выигрышу.
- [ ] A/B на speech_1/2/3 при -38/-26/-12 dBFS, fixed noise seeds. 15s повторения, одинаковые последние5s (10s initial adaptation), без выкидывания неудобных слов/пропусков. Проверить quiet on-off≥6dB, уменьшение межуровневого разброса≥6dB относительно off, loud settled≤off+3dB, finite/peak≤0.891252; отдельно учитывать уже clipped input.
- [ ] Проверить quiet→loud, loud→quiet, speech→pink/white noise2s→silence→speech, toggle с разной фазой сигнала и trim3/8/0. Noise last1s≤firstfull1s+3dB; zero exact; у fade последний sample попадает в enabled ceiling, adjacent discontinuity на low-amplitude control не превышает0.001.
- [ ] Изолированные AEC/NS control без AGC сохраняют ERLE≥20dB, double-talk loss≤6dB, stationary SNR improvement≥10dB/segmentalout≥in. Дополнительно full default-on прогон и calibrated same-window results, не приписывать AGC к SNR. Babble measured-only.
- [ ] Сохранить A/B PCM/WAV (input/off/on) и JSON/таблицу параметров, уровней/peaks/clipping/SNR. Если критерии не выполнены, править порядок/настройки upstream в пределах spec, повторять RED/GREEN; не ослаблять assertions и не возвращать custom compressor.
- [ ] Запустить voice_quality default-on с прежними onset/noise/network gates и три повторных latency runs поочерёдно, вне CPU-heavy suite. Обновить BASELINE действительными числами, environment/commit/командой и пределами synthetic fixtures.
- [ ] Commit `test(audio): measure native enhancement quality and regressions`.

### Task 7: Связь, старый сервер, нативный клиент и длительная работа

**Files:** Create `crates/finch-testbed/tests/voice_compatibility.rs`, при необходимости example `native-voice-probe`; modify `src/client.rs` для external endpoint; create/update `scripts/test-voice-soak.ps1`, QA/checklist.
**Interfaces:** `TestClient::connect_external(api_url, quic_addr, username, password, expected_fingerprint)` с отдельными тестовыми credentials; mandatory TLS fingerprint pin, без доверия произвольному production endpoint. `test-voice-soak.ps1 -Minutes 30 -OutputDirectory <isolatedpath>`.

- [ ] RED/GREEN e2e настоящий SFU+Opus: input fixtures → full on/off production capture → receiver mixer, обе стороны, mute/PTT/silence flags, reconnect, screen audio одновременно. Old helpers остаются default-on.
- [ ] Прогнать voice_quality/network_impairment/network_probe_metrics/screen_audio/voice_moderation/voice_signaling: ≤120ms normal, каждый impaired probe≤200ms, maxbuffer≤6, silentrun≤3. Ошибка второго/третьего probe не маскируется первым удачным.
- [ ] Скачать опубликованный v1.0.1 server ZIP, проверить SHA256SUMS, развернуть в своей папке на свободных портах, сверить fingerprint; два новых клиента, регистрация тестовых аккаунтов/приглашения, чат, QUIC voice в обе стороны. Затем те же проверки против нового server. Протокол/DB migrations не менять ради DSP.
- [ ] Нативный release client с двумя изолированными WebView профилями на текущем ПК: реальный вход, голос, настройка/toggle, запись3s/monitor, PTT/global key, mute/deafen, reconnect, выбор/исчезновение устройства, screen share+sound. Снять screenshots/error logs, сравнить UI и фактический PCM/пакеты.
- [ ] 30min soak с многократными PTT/toggle/rejoin и непрерывным capture, без дополнительной задержки/разрывов/crash. Зафиксировать private working set в steady-state на5й и30ймин: прирост≤20MiB, без монотонного frame-buffer growth; диагностические max/p95 DSP time, missed deadlines. Не писать WAV бесконечно в память.
- [ ] Полные `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo test --workspace --locked --offline -- --test-threads=1`, frontend tests/build.
- [ ] `cargo test -p finch-testbed --test load_50 --release --locked --offline -- --ignored --nocapture`: no disconnects, voice≥97%, video≥90%, chat≥99%, chatp95<500ms. Это transport load, не 50 параллельных DSP, указать в отчёте.
- [ ] В QA явно отделить два клиента/один ПК от двух ПК/LAN/NAT и прослушивания человеком. Commit `test(voice): validate native calls and server compatibility`.

### Task 8: Подготовить поставку1.1.0 и независимое ревью

**Files:** workspace Cargo.toml/lock; app/tauri.conf.json; frontend package.json/lock; workflows; `scripts/test-client-runtime.ps1`; `docs/THIRD-PARTY-NOTICES.md`, README, release notes/QA/checklists.
**Interfaces:** ровно1.1.0 во всех manifest/lock; installer `FinchSpeak_1.1.0_x64-setup.exe`; шесть assets прежнего состава. Native build version/hash входит в QA/provenance, не в UX.

- [ ] Обновить versions и notes-file в release workflow динамически по tag; одинаковый native prepare/provenance для CI и Release, cache key включает lock/toolchain/config/CRT. Native source/compiler не нужен в серверном ZIP.
- [ ] Build debug/release и NSIS с выбранным C++ runtime: не смешивать /MT и /MD в одной native link unit; проверять реальные PE imports клиента/всех поставляемых DLL. Допустимые VC runtime DLL включить через bundle; запретить нерешённые зависимости/зависимость от developer PATH. Серверный static CRT gate сохранить отдельно.
- [ ] Включить licenses/NOTICE/PATENTS/model notices в installer/portable/server где применимо; documented reproducible source/build и точный dependency list. Source acquisition hashes проверены, секретов нет в dist/logs.
- [ ] Заполнить QA доступными измерениями, pending publish checks явно пометить; release notes объясняют готовый AGC2, default-on/migration, ручной trim и limits, как найти Voice and audio и отключить новую обработку.
- [ ] Один свежий независимый reviewer всей ветки (самая сильная доступная модель согласно executing-plans): spec+plan+diff+логи, focus native ABI/ownership/normalized gain/privacy/metrics/runtime/updater. Не передавать секреты. Выполнить важные исправления с RED/GREEN и повторить затронутые проверки; все замечания получают явный disposition в QA/ledger.
- [ ] После исправлений повторить полный mandatory suite и clean release build. Generated JSON semantic-only изменения отдельно объяснить/исключить. Commit `chore(release): prepare FinchSpeak 1.1.0 with native audio`.

### Task 9: Установка, обновление, ZIP и публикация stable

**Files:** release scripts/packaging если обнаружены defects; final `docs/QA-1.1.0.md`, `docs/RELEASE-VALIDATION.md`, `docs/RELEASE-CHECKLIST.md`.
**Interfaces:** действующий updater public key, уже разрешённый secret `TAURI_SIGNING_PRIVATE_KEY`; GitHub tag v1.1.0 stable, public latest.json version1.1.0/platformwindows-x86_64, installer URL GitHub.

- [ ] Собрать server через `scripts/build-server-release.ps1`, client/NSIS и `scripts/package-release.ps1`; убедиться, что пути target не выбирают старые artifacts1.0.1 из общего cache. Version/provenance/hash проверять на пакетном payload.
- [ ] Fresh install1.1.0 и upgrade опубликованной1.0.1→1.1.0 в изолированном окружении/VM либо своей тестовой установке без затрагивания пользовательской; проверить version, native start, вход/voice и сохранение старых preferences/explicitfalse. Проверить uninstall и сохранность пользовательских данных по текущей политике.
- [ ] Проверить полный автоupdate flow: клиент1.0.1 читает настоящий подписанный manifest1.1.0, предлагает обновление, запускает installer после явного подтверждения в тестовом профиле, запускается1.1.0. До публикации использовать отдельный тестовый endpoint/сборку harness; production URL остаётся прежним. Если UI/system environment не позволяет, точно перечислить непроверенные шаги, не назвать полный flow PASS.
- [ ] Оба ZIP: Run_Server.bat после распаковки, пути с пробелами/кириллицей и CWD снаружи пакета; fresh/restart, readiness/log, HTTP/QUIC occupied, badconfig/TLS/missingexe. `test-server-launcher.ps1` и `test-release-package.ps1` — registration/invite/member/API/backup/restore, только test data. Проверить portable client запускается из ZIP payload.
- [ ] Подписать installer существующим ключом без печати; make-latest-json + `verify-release` с built-in public key: genuine PASS, tamper rejected, manifest version/URL/signature match. SHA256SUMS соответствует обоимZIP, installer, sig, manifest. Signing env очищать в finally.
- [ ] Интегрировать проверенную ветку в main обычным merge/ff без force; push main; дождаться CI SUCCESS. Создать неизменяемый tagv1.1.0 и push; дождаться Release SUCCESS. Не создавать повторно/перемещать тег при failure; описать failure и исправить стратегию версии до дальнейшей публикации.
- [ ] Проверить stable/latest GitHub release и все6 assets; скачать в свежую папку, проверить пять SHA256 по SHA256SUMS, genuine/tamper signatures, version metadata, native client smoke и оба опубликованныхZIP launchers. Public latest.json анонимно доступен и предлагает1.1.0 старому клиенту; равному/новому не предлагает downgrade.
- [ ] Обновить QA реальными URLs/run IDs/commit/artifact hashes/PASS/limits, final docs commit с `[skip ci]` если код не менялся. Пользователю дать ссылку на release, короткий LAN-гайд и оставшиеся OWNER CHECK для двух ПК/микрофонов, без фиктивных claims.

## Definition of done и правило остановки

Готово = original WebRTC и RNNoise реально используются установленным клиентом1.1.0; own DSP/Sonora удалены; все доступные обязательные gates зелёные; review закрыт; native installation/ZIP/signing проверены; stable tag+GitHub jobs SUCCESS и скачанные опубликованные assets проверены; QA честно отмечает внешнюю ручную приёмку.

Блокирующий build/quality/privacy/runtime regression не обходится fallback-DSP, пропуском тестов или ослаблением gates. Исправления внутри архитектуры автономны. Невозможность соблюсти выбранную библиотеку или продуктовый контракт выносится владельцу конкретным отчётом. Два физических ПК и субъективное прослушивание остаются владельцу, если у исполнителя нет этих ресурсов; обещание одинакового звучания Discord/TeamSpeak не используется как тест.

## Self-review и traceability

- Spec Библиотеки/поставка → Tasks1,2,8,9; Тракт → Tasks2–4; Профиль → Tasks2,6; UX/privacy → Tasks4,5,7; QA criteria1–10 → Tasks1–9.
- Единственные новые fallible interfaces определены в Tasks2/3; worker/testbed callers обновляются до запуска полного workspace suite. Предыдущие DTO/поле и имя toggle сохраняются.
- Критерии отличия AGC2 от custom-DSP описаны явно, не выдаются за неизменный прежний compressor; old released quality gates сохранены и отделены от gain-biased SNR.
- Build feasibility — первая законченная задача, а не предположение последнего шага. Реализация и публикация не начаты этим документом.