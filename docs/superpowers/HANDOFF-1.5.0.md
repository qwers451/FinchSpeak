# HANDOFF: FinchSpeak 1.5.0

Дата: 2026-11-10. Для агента-исполнителя. **Что делать и как именно** — в
[спецификации](specs/2026-11-09-release-1.5.0-design.md): там приняты все решения, включая форматы сообщений, ключи
хранилищ, тексты строк и критерии приёмки. Этот документ — состояние, среда, карта кода и порядок сдачи.
Ревью после каждого пакета делает другой агент.

## 1. Состояние репозитория

- Ветка `main` = `origin/main` = `5f44fe2` (релиз 1.4.1). Работать в ветке `feat/1.5.0` от `main`.
- Не закоммичено (закоммитить первым коммитом `docs: specification and handoff for 1.5.0`, кроме теста):
  - `docs/superpowers/specs/2026-11-09-release-1.5.0-design.md`
  - `docs/superpowers/HANDOFF-1.5.0.md`
  - `crates/finch-testbed/tests/state.rs` — добавлен тест `state_lists_people_who_are_not_connected_as_offline`.
    Он **не компилируется**, пока нет поля `offline_users` (спецификация, 1.2). Не запускался. Коммитится вместе с
    исправлением 1.2.
- `deploy-linux-1.4.0/` и `promo/` — чужие неотслеживаемые папки. Не трогать, не коммитить, не добавлять в `.gitignore`.
- Код продукта не менялся.
- Существуют отдельные рабочие копии других веток (`FinchSpeak-docs`, `-motion`, `-ux`, `-docker`, `-audio` рядом с
  репозиторием) — не трогать.

## 2. Среда сборки

Проверено 2026-11-10:
- Rust и MSVC на машине отсутствовали. Запущена установка через winget:
  - `Rustlang.Rustup` — **установлен** (winget сообщил об успехе). В уже открытых терминалах `cargo` в PATH нет:
    открыть новый PowerShell или добавить `$env:USERPROFILE\.cargo\bin` в `$env:PATH` на сессию.
  - `Microsoft.VisualStudio.2022.BuildTools` с `Microsoft.VisualStudio.Workload.VCTools` — **установлен**:
    `vswhere … -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64` возвращает
    `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`.
  - `cargo 1.99.1` отвечает по пути `$env:USERPROFILE\.cargo\bin\cargo.exe`. Сборка проекта ещё **не запускалась**.
- Git Bash: `cargo` и рабочего `python` нет. Все Rust-команды — из PowerShell. Смена каталога в Bash сохраняется между
  вызовами — использовать абсолютные пути.
- `python` в PATH — заглушка Microsoft Store. Настоящий Python 3.13.3 нужен только для `scripts/prepare-webrtc.ps1`.
- Нативный WebRTC ([NATIVE-AUDIO.md](../NATIVE-AUDIO.md)): в `target/native/{Debug,Release}` и `target/native-cache`
  лежат артефакты от 3–4 ноября. Сначала попробовать их: в PowerShell с окружением MSVC
  (`Launch-VsDevShell.ps1 -Arch amd64` или `vcvars64.bat`) выполнить
  `./scripts/prepare-webrtc.ps1 -Configuration Debug -Offline`. Скрипт выставляет `FINCH_WEBRTC_ROOT` для текущего
  процесса PowerShell — `cargo` запускать в том же процессе. Если без Python не проходит — остановиться и спросить
  владельца про установку Python 3.13.3 (`winget install Python.Python.3.13`).
- Node есть (`C:\Program Files\nodejs`), `app/frontend/node_modules` установлен.
- Свободно на C: около 34 ГБ до установки Build Tools; сборка workspace в debug занимает несколько ГБ в `target/`.

Запуск двух клиентов для проверки (из [ROADMAP](../ROADMAP.md)): сервер — `Run_Server.bat`; каждый клиент со своей
`WEBVIEW2_USER_DATA_FOLDER`; для автоматизации окна — WebView2 с `--remote-debugging-port`.

## 3. Карта кода по пакетам

| Пакет | Основные файлы |
| --- | --- |
| 1.1 логотип | `app/frontend/src/ui/Titlebar.tsx`, `Titlebar.test.tsx` |
| 1.2 офлайн-участники | `crates/finch-protocol/src/models.rs` (`ServerState`), `crates/finch-server/src/api/state.rs`, `crates/finch-client-core/tests/control_stream_test.rs:36`, `app/frontend/src/types.ts`, `stores/sessionStore.ts:72-94,150-158`, `components/MembersPanel.tsx`, `crates/finch-testbed/tests/state.rs` |
| 1.3 звук стрима | `components/screen/SharePickerModal.tsx:59`, `screenUi.test.tsx` |
| 2.1 трей | `app/src/tray.rs`, `app/src/commands/window.rs` (`set_tray_state`), `stores/desktop.ts:91-98,119-132`, `services/api.ts:532` |
| 2.2 анимация | `ui/Avatar.tsx`, `styles/motion.css:32-44`, `finch.css:145-147`, `themes.css:70,240`, `polish.css:151`, `stores/settingsStore.ts`, `components/settings/SettingsModal.tsx`, `app/src/audio_engine.rs` (события `user-speaking` ~896, `local-speaking` ~996-1105), `crates/finch-audio/src/mixer.rs` |
| 3 стрим | `crates/finch-screen/src/capture.rs`, `hw_encoder.rs` (OpenH264, не аппаратный), `crates/finch-client-core/src/screen.rs` (`ScreenPacketizer`, `ScreenSharer`, `fit_to_bgra`, `default_bitrate`, `encoded_size`), `app/src/commands/screen.rs` (`SHARE_FPS`, `list_screen_sources`, `start_screen_share`, `register_video_channel`), `app/src/state.rs` (`VideoChunkPayload`), `components/screen/ViewerPanel.tsx`, `SharePickerModal.tsx`, `SharerPill.tsx` |
| 4 люди | `components/voice/ParticipantTile.tsx`, `components/Sidebar.tsx:180-190`, `components/VoicePanel.tsx`, `components/voice/voiceLayout.ts`, `stores/volumeStore.ts`, `stores/adminStore.ts:165-180`, новый `ui/ContextMenu.tsx` |
| 5 полировка | `components/voice/VoiceSettingsModal.tsx:23`, `components/settings/ThemeEditor.tsx:29`, `styles/*` |
| 6 активность | новый `app/src/activity.rs`, `crates/finch-protocol/src/messages.rs`, `models.rs`, `crates/finch-server/src/quic.rs` (`handle_client_message`), `session.rs`, `components/MembersPanel.tsx`, `ParticipantTile.tsx` |
| 7 музыка | новые `crates/finch-server/src/api/music.rs`, `migrations/012_music.sql`, `crates/finch-audio/src/music.rs`, `app/src/music.rs`, `components/music/*`, `stores/musicStore.ts`; существующие `crates/finch-server/src/config.rs`, `api/mod.rs` (маршруты), `api/files.rs` (образец загрузки), `session.rs` (`VoiceRoom`), `sfu.rs` (не менять), `app/src/audio_engine.rs` (`mix_next_frame` ~331, `render_playback` ~341, `render_deafened` ~364), `components/voice/Dock.tsx` |

Что полезно знать заранее:
- Управляющий протокол — MessagePack с именованными полями (`codec.rs:39`, `to_vec_named`). Совместимость новых
  сообщений — раздел 0.1 спецификации (`client_level`, `ServerState.features`).
- Звук стрима идёт типом `MediaType::ScreenAudio`, сервер шлёт его только подписчикам (`sfu.rs:80`), микшер сводит его в
  моно (`mixer.rs:203`). Музыка этим путём **не** идёт (спецификация, 7.1).
- Существующие загрузки из фронтенда передают файл как `number[]` (`api.ts:160`) — для музыки так нельзя (7.3).
- В `tauri.conf.json` уже `dragDropEnabled: false`: HTML5 drag-and-drop в окне работает.
- Темы: `claude`, `custom`, `user` (`prefs.appearance.theme`); у `custom` срез углов через списки селекторов в
  `themes.css:209-222` — новые «карточные» элементы добавлять туда.
- В документе [UI-MOTION-2026-11.md](../UI-MOTION-2026-11.md) записан прежний запрет на циклические анимации; спецификация
  (2.2) осознанно разрешает их для индикатора речи — документ обновить.

## 4. Порядок работы и сдача

1. Среда (раздел 2). Затем базовая линия всех пяти проверок на чистом `main` — вывод сохранить. Красную базовую линию
   не чинить молча: сообщить.
2. Пакеты строго по порядку: 1 → 2 → 3 → 4 → 5 → 6 → 7 → релизные документы. Внутри пакета — по пунктам спецификации.
3. Один пункт — один коммит, тест первым. Сообщения — в стиле репозитория (`fix:`, `feat:`, `ui:`, `docs:`, `test:`).
4. После каждого пакета — остановка и отчёт ревьюеру:
   - коммиты (хеш, заголовок);
   - вывод пяти проверок (последние строки каждой);
   - что проверено в запущенном приложении и как именно;
   - что **не** проверено и почему — отдельным списком для владельца всё, что требует слуха, второго ПК, реальной игры;
   - отклонения от спецификации с причиной (их быть не должно без остановки и вопроса).
5. Запрещено до завершающего ревью: push в `origin`, смена версии, теги, слияние в `main`.
6. Не писать «работает», «исправлено», «проверено» без запуска и вывода команды в том же отчёте.
