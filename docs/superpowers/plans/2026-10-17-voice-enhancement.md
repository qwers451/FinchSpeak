# Voice Enhancement 1.1.0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Альтернатива по выбору владельца: superpowers:subagent-driven-development.

**Goal:** Сделать голос участников ровнее и комфортнее через автоматическое усиление, мягкую компрессию и ограничение пиков, затем автономно выпустить стабильный Windows-релиз 1.1.0.

**Architecture:** Обработка микрофона выполняется на клиенте до Opus, отдельно для каждого отправителя. Существующие AEC, RNNoise, VAD и pre-roll дополняются чистым Rust DSP-модулем; настройки передаются аудиопотоку существующим механизмом atomics. SFU, формат пакетов и обработка звука демонстрации экрана сохраняют совместимость.

**Tech Stack:** Rust/f32, finch-audio, существующие Sonora AEC3 / nnnoiseless / Earshot / Opus, Tauri 2, SolidJS, Vitest, Windows NSIS, GitHub Actions.

**Spec:** Согласованная в чате схема от 2026-10-17 и раздел «Согласованные решения и контракт» этого файла. До итогового подтверждения владельца это план для согласования, а не разрешение начинать реализацию.

## Согласованные решения и контракт

Владелец выбрал:
- Естественный голос с мягким выравниванием, сохранением интонации.
- Один переключатель улучшения и существующие настройки микрофона.
- Сразу стабильный `v1.1.0`, доступный существующим клиентам через механизм обновления.

Предлагаемые детали, утверждаемые вместе с этим планом:
- `voice_enhancement: bool`, по умолчанию `true`, включая миграцию настроек 1.0.1. Явное `false` сохраняется после перезапуска.
- Переключатель в `Voice and audio`: `Voice enhancement`, пояснение `Balances quiet and loud speech and limits peaks.` Язык существующего интерфейса остаётся английским.
- Переключатель управляет новой AGC/компрессией/лимитером; существующее подавление эха и шума работает и в выключенном режиме. Это отключение новой обработки, а не режим сырого микрофона.
- Обработка применяется к собственному микрофону каждого обновлённого клиента. Для общего эффекта участникам нужны клиенты 1.1.0; сервер 1.0.1 должен оставаться совместимым. Клиент не меняет настройки других пользователей принудительно.
- `Input volume` сохраняется как ручной входной trim; AGC частично компенсирует его. Значение `0` означает тишину. `Speech threshold` остаётся пользовательским порогом открытия микрофона: AGC его не обходит. Речь ниже этого порога требует уменьшения порога/настройки источника; усилитель не восстанавливает уже потерянные данные.
- AEC и NS получают исходный нормализованный сигнал. Ручное усиление переносится после AEC/NS, перед VAD и новой динамической обработкой, чтобы программное усиление не обрезало пики до компрессора. При громком сигнале для анализа VAD используется отдельная ограниченная копия; аудиосигнал сохраняет запас уровня до лимитера. Изменение порядка явно описать в release notes.
- Индикатор уровня и линия порога относятся к сигналу перед AGC/компрессией, по которому принимается решение VAD. `Test microphone` и `Hear myself` воспроизводят конечный обработанный сигнал.
- Существующие 80 мс pre-roll и 100 мс hangover сохраняются. Закрытый PTT, mute и тест микрофона не должны оставлять в сетевом pre-roll звук, который затем уйдёт собеседникам.
- Никаких новых фоновых сервисов, DSP-плагинов, моделей или загрузок через интернет для работы обработки.

### Исходный естественный профиль

Это инженерные начальные значения, а не обещание одинакового тембра всех микрофонов:

| Параметр | Значение |
|---|---|
| Целевая RMS-громкость распознанной речи до компрессора | −22 dBFS |
| Диапазон автоматического усиления | −18…+12 dB |
| Скорость увеличения/уменьшения AGC | до 6 / 18 dB в секунду |
| Компрессор: threshold / ratio / knee | −18 dBFS / 3:1 / 6 dB |
| Компрессор: attack / release | 5 / 120 мс |
| Лимитер: peak ceiling / release | −1 dBFS / 50 мс |
| Дополнительный look-ahead и очередь кадров | 0 |
| Сглаживание переключения режима | 20 мс, без добавления кадров задержки |

AGC адаптируется только на `Speaking`, замораживает усиление на `Hangover` и `Silence`; компрессор и лимитер продолжают ограничивать пики. Плавность gain обеспечивается внутри кадра. Новый сеанс/смена устройства сбрасывает историю DSP. Не использовать жёсткое обнуление внутри слов как дополнительный noise gate.

Разрешена автономная настройка естественного профиля по измерениям: target −24…−20 dBFS, ratio 2:1…3:1, knee 4…8 dB, attack 3…10 мс, release 100…200 мс. Максимальное усиление +12 dB и потолок −1 dBFS не повышать. Итоговые значения зафиксировать в коде и QA-отчёте. Пороги приёмки ниже не ослаблять ради зелёных тестов.

## Global Constraints

- Windows x64, stable release `v1.1.0`; старые опубликованные теги/артефакты не перезаписывать.
- Моно 48 kHz, кадр 960 samples / 20 мс, текущие Opus и формат MediaHeader без изменений.
- Новые DSP-операции без аллокаций, блокировок и I/O в обработке кадра. В cpal callbacks не добавлять обработку: она остаётся в audio worker.
- Новые зависимости по умолчанию запрещены: достаточно существующего стека и Rust std.
- Сохранить действующие критерии AEC/NS/VAD, конца/начала речи, обычной задержки ≤120 мс и сетевого impaired-профиля ≤200 мс, буфера ≤6 пакетов и последовательной тишины ≤3 кадров.
- Пользовательские БД, сертификаты, секреты и каталоги работающих серверов не изменять. Все проверки — в отдельных папках и на свободных портах; закрывать только собственные процессы.
- Для подписи разрешены существующий updater.key и настроенный GitHub secret. Ключ не печатать и не коммитить. Authenticode не входит в работу.
- После утверждения плана инженерные решения, исправления найденных регрессий, тесты, коммиты, push и публикация выполняются автономно. Изменения продуктового объёма/ослабление критериев требуют нового согласования.

## Review Focus

- Тихая речь/шум и паузы: AGC не раскачивает фон и не обходит заданный пользователем порог; Task 1/2/4.
- Уже громкий микрофон, Input volume 300%, ноль и нечисловые samples: нет предварительного цифрового обрезания из-за trim, переполнения и NaN; Task 1/2.
- Pre-roll после закрытого PTT, mute и mic test: приватный звук не появляется в последующих пакетах; Task 2/4.
- Переключение режима/смена устройства/переподключение: нет выброса накопленного gain, пропавшего первого слова и старой записи; Task 1/2/3.
- Сохранённое false, отсутствующее поле, отказ команды и старый сервер: предпочтения сохраняются, UI не сообщает ложный успех, протокол совместим; Task 3/4/5.

## Файлы и ответственность

- Create `crates/finch-audio/src/dynamics.rs`: AGC, compressor, limiter, сглаживание и unit tests.
- Modify `crates/finch-audio/src/lib.rs`, `capture.rs`: экспорт, подключение DSP, порядок trim, pre-roll и анализ VAD.
- Modify `app/src/state.rs`, `audio_engine.rs`, `commands/voice.rs`: DTO/atomics и применение режима в worker.
- Modify `app/frontend/src/types.ts`, `services/api.ts`, `stores/voiceStore.ts`, `components/voice/VoiceSettingsModal.tsx`: поле, default/migration, UI.
- Modify соответствующие Rust/TS tests; create `crates/finch-testbed/tests/voice_enhancement.rs`; modify `crates/finch-testbed/src/voice.rs` и `BASELINE.md` для A/B-проверок.
- Modify версии в workspace Cargo.toml/Cargo.lock и Tauri/frontend package files; `.github/workflows/release.yml` — правильный notes file для 1.1.0.
- Create `docs/RELEASE-NOTES-1.1.0.md`, `docs/QA-1.1.0.md`; обновить README/checklist.

### Task 1: Модуль динамической обработки

**Interfaces:** `VoiceDynamics::new() -> Self`, `set_enabled(&mut self, bool)`, `reset(&mut self)`, `process(&mut self, &mut [f32; FRAME_SIZE_48K], VadDecision)`. Единственный профиль — константы из контракта; telemetry для тестов через private поля/unit tests, без публичных DSP-настроек.

- [ ] Написать RED unit tests `quiet_speech_is_raised`, `loud_speech_is_reduced`, `gain_freezes_on_non_speech`, `peaks_are_limited`, `non_finite_samples_are_sanitized`, `zero_input_stays_zero`, `toggle_is_smoothed`, `reset_forgets_gain`.
- [ ] Зафиксировать assertions: на повторяемом voiced-сигнале RMS −38 dBFS прибавка ≥6 dB после 3 с; RMS −12 dBFS ослабляется ≥6 dB после 3 с; разница стабильных уровней между одинаковым сигналом −38/−26/−12 dBFS ≤8 dB. Все samples finite, peak ≤`10^(-1/20)+1e-6`. В 2 с Silence/Hangover AGC gain не увеличивается. После reset состояние соответствует новому объекту.

Пример обязательных assertions; test-local helper `render_case(input_db: f32, seconds: u32, decision: VadDecision) -> (Vec<f32>, f32)` подаёт непрерывный синус 200 Hz и возвращает output/его RMS за последнюю секунду:
```rust
#[test]
fn quiet_speech_is_raised() {
    let (out, settled_db) = render_case(-38.0, 3, VadDecision::Speaking);
    assert!(settled_db >= -32.0);
    assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 10f32.powf(-1.0 / 20.0) + 1e-6));
}
#[test]
fn loud_speech_is_reduced() {
    let (_, settled_db) = render_case(-12.0, 3, VadDecision::Speaking);
    assert!(settled_db <= -18.0);
}
```
- [ ] Запустить `cargo test -p finch-audio dynamics --offline`, подтвердить ожидаемые FAIL.
- [ ] Реализовать ф32 soft-knee compressor со сглаженным gain, speech-gated AGC, limiter с мгновенной атакой и плавным восстановлением; переключение dry/wet в пределах текущего кадра. Без look-ahead, resampling и изменения числа samples.
- [ ] Повторить tests; измерить обработку 60 с сигнала и записать CPU/time как диагностику, без хрупкого аппаратно-зависимого порога CI.
- [ ] Коммит `feat(audio): add natural voice dynamics`.

### Task 2: Подключение к capture и сохранение контроля передачи

**Interfaces:** добавить `VoiceCaptureConfig.voice_enhancement: bool = true` и `VoiceCapture::set_voice_enhancement(&mut self, bool)`. `CaptureOutput.level_db` сохраняет смысл уровня, сравниваемого с VAD threshold. Поле сетевого протокола не добавлять.

- [ ] RED capture tests: default true; явное false; trim=0; trim=3 на громком входе; RMS meter соответствует VAD; quiet speech выше порога передаётся; ниже порога не открывает микрофон; normal VAD pre-roll/hangover сохраняются.
- [ ] RED privacy tests: после закрытого transmit gate/микрофонного теста и повторного открытия в новых пакетах отсутствует узнаваемый предыдущий сигнал; mic-test PCM при этом содержит конечную обработку.
- [ ] Встроить порядок `AEC → NS → manual trim → VAD analysis → dynamics → pre-roll/Opus`. До AEC заменять нечисловые input samples нулём и ограничивать исходный PCM допустимым диапазоном. Для VAD ограничивать копию samples, не обрезать trim-сигнал перед dynamics. В выключенном режиме после trim применять прежнюю допустимую границу PCM. При input_gain=1 и enhancement=false сохранить прежний тракт AEC/NS/VAD/Opus. Обновить все явные литералы VoiceCaptureConfig в workspace, включая тесты.
- [ ] Разделить реальную активность речи и разрешение сетевой передачи: AGC может работать для локального прослушивания, закрытый gate очищает сетевой pre-roll. Сбрасывать DSP/pre-roll при смене источника и новом сеансе.
- [ ] Запустить `cargo test -p finch-audio --offline`; существующие tests изолированных функций, которым нужна передача без dynamics, явно пометить enhancement=false, сохранив отдельные обязательные default-on tests. Не переводить весь старый quality-набор на false.
- [ ] Коммит `feat(audio): integrate enhancement with capture and voice gating`.

### Task 3: Переключатель, сохранение и применение настроек

**Interfaces:** `VoiceSettingsDto.voice_enhancement: bool` с serde default=true; одноимённые `AtomicBool`, getter и TS `VoiceSettings` property. Текущая `set_voice_settings` сохраняет интерфейс команды.

- [ ] RED Rust settings tests: default true, apply false/true, round-trip snapshot; десериализация старого DTO без поля даёт true.

Контракт миграции backend закрепить тестом (старый DTO не содержит нового поля):
```rust
let old = r#"{"input_gain":1.5,"vad_threshold_db":-50,"push_to_talk":false,"key_down":false,"monitor":false}"#;
let settings: VoiceSettingsDto = serde_json::from_str(old).unwrap();
assert!(settings.voice_enhancement);
assert_eq!(settings.input_gain, 1.5);
assert_eq!(settings.vad_threshold_db, -50.0);
```
- [ ] RED Vitest: миграция старого saved JSON сохраняет gain/threshold/PTT/key/monitor; отсутствующее/испорченное новое поле → true, явное false → false; ошибка API не меняет локальный переключатель и storage.
- [ ] Расширить DTO, atomics, frontend type/API mock/default/restore. Audio worker читает atomic каждый кадр и вызывает setter без перестроения устройства.
- [ ] Добавить один Toggle в `Voice and audio`, с указанными label/description; текущие Input volume, threshold, PTT, hear myself и mic test доступны. Добавить hint: индикатор относится к открытию микрофона, автоматическое усиление применяется после распознавания речи.
- [ ] RED/GREEN component test: переключатель включён по умолчанию, отправляет новое значение, корректно восстанавливает false после перезапуска; микрофонный тест и monitoring используют этот режим.
- [ ] Запустить `cargo test -p finch-app --lib --offline`, frontend `npm test -- --run`, `npm run build`.
- [ ] Коммит `feat(app): persist and expose voice enhancement`.

### Task 4: Объективное качество, голосовая связь и совместимость

**Interfaces:** добавить `run_voice_link_with_config(sender: &TestClient, receiver: &mut TestClient, input: &[f32], tail_frames: usize, config: VoiceCaptureConfig) -> anyhow::Result<VoiceRun>`; текущие helpers делегируют ему, сохраняя default-on. Для offline A/B использовать один и тот же speech fixture и noise seed.

- [ ] RED end-to-end tests в `voice_enhancement.rs`: три существующих speech fixtures, уровни −38/−26/−12 dBFS, скачок тихо→громко, речь→шум→пауза→речь, переключение on/off, PTT и silence flags.
- [ ] Проверить новый профиль в default-on: мягкое сближение уровней (разница ≤8 dB после стабилизации), отсутствие NaN/перегрузки, отсутствие роста фона на паузе и новой очереди аудио. Сравнивать одинаковые voiced-участки, пропуская только явно заданный 3-секундный интервал адаптации AGC, не исчезнувшие/неудобные слова.
- [ ] Выполнить действующие `voice_quality`, `network_impairment`, `network_probe_metrics`, `screen_audio`, `voice_moderation`, `voice_signaling` с текущими критериями. Порог NS +10 dB остаётся для белого/розового шума; babble измеряется по прежнему решению владельца. При влиянии динамики на измеритель адаптировать метод через калибровку gain/контрольные A/B-прогоны с отдельной проверкой default-on, не ослабляя порог и не скрывая тест отключением функции.
- [ ] Не засчитывать усиление speech-участка относительно отдельной тихой паузы как прирост SNR от шумоподавления: новый измеритель должен быть инвариантен к общему постоянному gain, учитывать одинаковые voiced-окна/калибровку gain в сравниваемых clean/noise данных. Исходную функцию metrics не переписывать ради нужного числа. Изолированный AEC/NS-прогон с dynamics=false допустим только вместе с проверками полного default-on тракта; оба результата публикуются.
- [ ] Сохранить WAV A/B в `target/voice-enhancement-out` и метрики в `BASELINE.md`/QA. Автоматическая оценка не выдаётся за прослушивание человеком.
- [ ] Проверить новый нативный клиент с опубликованным сервером 1.0.1 в отдельной папке: вход, чат, QUIC voice в обе стороны; сервер и аккаунты тестовые. Сверить TLS перед доверием. При недоступности второго ПК провести два клиента на текущем ПК и явно отметить предел проверки.
- [ ] Полные checks: `cargo check --workspace --all-targets --locked --offline`; `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`; `cargo test --workspace --locked --offline -- --test-threads=1`; frontend test/build; отдельно `cargo test -p finch-testbed --test load_50 --release --locked --offline -- --ignored --nocapture`.
- [ ] Независимое ревью всей ветки, исправление блокирующих замечаний с регрессионными tests. Коммит `test(audio): verify enhanced voice and compatibility`.

### Task 5: Стабильный выпуск 1.1.0 и проверка опубликованных файлов

- [ ] Поднять Cargo/Tauri/frontend версии до 1.1.0, обновить lockfiles. Workflow выбирает `docs/RELEASE-NOTES-${version}.md` и установщик строго по версии тега. Сохранить static-CRT server gate и обе проверки BAT.
- [ ] Notes/QA описывают default-on, выключение, связь Input volume/threshold/AGC, совместимость сервера 1.0.1, измерения и пределы проверки. Дать владельцу A/B-чеклист для реальных микрофонов/наушников на двух ПК.
- [ ] Собрать NSIS, portable и server ZIP. Проверить переключатель, сохранение false, обработанный mic test и UI нативного установленного клиента. Проверить upgrade 1.0.1→1.1.0 в собственной тестовой установке и её удаление, сохранив пользовательские данные.
- [ ] Перед публикацией должны пройти все обязательные автоматические проверки. Если критерий не выполняется — исправлять; не выпускать заведомо падающую версию и не ослаблять критерий. Не запрашивать повторное разрешение на заранее согласованные коммиты/push/подпись/публикацию.
- [ ] Коммит release preparation, push и новый стабильный тег `v1.1.0`. Существующие опубликованные теги 1.0.0/1.0.1 не менять. Подписать разрешённым ключом через существующий workflow, дождаться успешных CI/release.
- [ ] Заново скачать все assets: SHA-256, встроенный public key, отказ изменённому installer, оба BAT, PE runtime imports, API, restart/backup/restore. Публичный latest.json без авторизации обязан указывать на 1.1.0 и правильный installer.
- [ ] Записать workflow URLs, commit SHA, итоговые counts и результаты в `docs/QA-1.1.0.md`; commit/push документации. Завершить собственные процессы/тестовые установки; сохранить отчёты и резервные копии.

## Готовность и автономная работа

Рекомендуемый способ: реализация самим основным агентом в текущем чате, отдельная ветка `codex/voice-enhancement`/изолированный checkout при наличии поддерживаемого механизма, затем независимое итоговое ревью. Текущий стабильный сервер владельца остаётся рабочим во время разработки.

После утверждения этого файла дополнительных согласований обычной реализации не требуется. Основной результат — стабильный GitHub Release 1.1.0 и воспроизводимый QA-отчёт. Реальную субъективную оценку голоса на двух ПК владелец выполняет после выпуска; она не подменяется автоматическими метриками. Возврат к 1.0.1 возможен с сохранением данных сервера; обратное автообновление клиентам не предлагается.
