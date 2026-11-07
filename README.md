<p align="center">
  <img src="app/frontend/public/custom/assets/brand/finch-mark.svg" alt="FinchSpeak" width="120" height="82">
</p>

<h1 align="center">FinchSpeak</h1>

<p align="center"><strong>Свой сервер. Свои разговоры.</strong><br>
Голосовые комнаты, чат и демонстрация экрана для Windows — на вашем собственном сервере.</p>

<p align="center">
  <a href="https://github.com/qwers451/FinchSpeak/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/qwers451/FinchSpeak?label=%D1%80%D0%B5%D0%BB%D0%B8%D0%B7"></a>
  <a href="https://github.com/qwers451/FinchSpeak/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/qwers451/FinchSpeak/ci.yml?branch=main&label=CI"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/github/license/qwers451/FinchSpeak"></a>
  <img alt="Windows" src="https://img.shields.io/badge/platform-Windows-0078D6?logo=windows">
  <img alt="Rust + Tauri 2" src="https://img.shields.io/badge/Rust%20%C2%B7%20Tauri%202-SolidJS-orange?logo=rust">
</p>

<p align="center">
  <a href="https://github.com/qwers451/FinchSpeak/releases/latest"><b>Скачать</b></a> ·
  <a href="docs/RELEASE-NOTES-1.4.0.md">Что нового в 1.4.0</a> ·
  <a href="docs/DEPLOY-LINUX.md">Сервер на Linux</a> ·
  <a href="docs/RELEASE-CHECKLIST.md">Проверка на двух ПК</a>
</p>

---

## Почему FinchSpeak

- 🏠 **Самохостинг** — сервер ваш: история, файлы и аккаунты никуда не уходят.
- 🎙️ **Чистый голос** — шумоподавление WebRTC и RNNoise, кодек Opus; сервер пересылает звук без перекодирования.
- 🖥️ **Демонстрация экрана со звуком** — зрители слышат игру или видео, но не самих себя; можно передавать звук только одного окна.
- 🔐 **Как в Discord каждый день** — вход сохраняется, автозапуск с Windows, работа из трея, звуки входа, выхода и микрофона, глобальные клавиши mute/deafen.
- 🔗 **Приглашения ссылкой** `finchspeak://join` — сервер, код и отпечаток сертификата в одной ссылке.
- 💬 **Текстовые каналы** с ответами, реакциями, упоминаниями, поиском и вложениями.
- 🛡️ **Администрирование** — роли, модерация, приглашения, устройства пользователей, сброс пароля, резервные копии.
- 🎨 Две темы (**Claude Dark** и **Finch Custom**), русский и английский интерфейс, анимации с учётом «Уменьшить движение».

## Быстрый старт

### Участнику

Установите `FinchSpeak_<версия>_x64-setup.exe` из [Releases](https://github.com/qwers451/FinchSpeak/releases/latest) и откройте ссылку-приглашение — или введите адрес сервера `https://АДРЕС:8443` и код приглашения. При первом подключении сверьте отпечаток сертификата с тем, что сообщил владелец сервера.

### Владельцу сервера — Windows

1. Распакуйте `FinchSpeak-server-windows.zip` в папку с правом записи и запустите `Run_Server.bat`.
2. Откройте в брандмауэре **TCP 8443** (вход, чат, файлы) и **UDP 4433** (голос и видео); для доступа из интернета пробросьте их на роутере.
3. Сообщите участникам адрес и отпечаток сертификата из окна сервера.
4. Первый зарегистрированный аккаунт становится администратором; остальных зовите через **Администрирование → Приглашения**.

### Владельцу сервера — Linux (Docker)

```bash
git clone --depth 1 https://github.com/qwers451/FinchSpeak.git && cd FinchSpeak/deploy
docker compose up -d --build
docker compose logs | grep fingerprint
```

Сборка образа компилирует Rust (на одном ядре — 20–40 минут). Для маленького VPS удобнее собрать образ на другом компьютере и перенести одним файлом — см. [docs/DEPLOY-LINUX.md](docs/DEPLOY-LINUX.md), там же настройка портов, файрвола и обновление.

## Требования к серверу

| Ресурс | Сколько нужно |
| --- | --- |
| Процессор | Одного ядра достаточно: сервер пересылает медиа без перекодирования |
| Память | 10–60 МБ (замер: 50 клиентов в одной комнате — 48 МБ на весь процесс) |
| Диск | База и сертификат — мало; место занимают вложения (до 50 МБ на файл) и резервные копии |
| Канал | Голос ≈ 0,1 Мбит/с на каждого слушателя, экран 1080p30 ≈ 5 Мбит/с на каждого зрителя |
| Сеть | Публичный IP (или проброс портов), открытые TCP 8443 и UDP 4433 |

## Данные и обновления

- **Windows:** база, вложения, журналы, резервные копии и сертификат лежат рядом с `finch-server.exe`. Перед обновлением остановите сервер и сохраните **всю папку** (особенно `finchspeak.db`, `data/` и `finch-server.toml`), затем замените `finch-server.exe` и запустите `Run_Server.bat`.
- **Docker:** всё хранится в томе `finch-data`; обновление — новый образ и `docker compose up -d`, данные и отпечаток сохраняются. Не удаляйте том.
- Резервная копия базы делается автоматически раз в сутки (по умолчанию хранятся последние 7).
- Клиенты обновляются сами: приложение проверяет подписанные обновления в GitHub Releases.
- Обновляйте сервер вместе с клиентами — заметки к каждой версии в [`docs/`](docs).

## Безопасность

- Соединение защищено **TLS 1.3**; сертификат сервера закрепляется при первом подключении (TOFU), его смена показывается предупреждением.
- Токен входа хранится в **диспетчере учётных данных Windows**, пароль не сохраняется.
- Сквозного шифрования между участниками нет: сервер и его данные принадлежат администратору.
- Установщик не подписан Authenticode, поэтому Windows может показать SmartScreen. Скачивайте только из [Releases](https://github.com/qwers451/FinchSpeak/releases) и сверяйте `SHA256SUMS.txt` из того же выпуска.

## Как устроено

```
Клиент (Tauri 2 + SolidJS) ──HTTPS :8443 / QUIC :4433──► Сервер (Rust SFU + SQLite)
```

| Каталог | Что внутри |
| --- | --- |
| [`app`](app) | Настольный клиент: бэкенд Tauri на Rust и интерфейс на SolidJS |
| [`crates/finch-server`](crates/finch-server) | Сервер: QUIC SFU, REST API, чат, резервные копии |
| [`crates/finch-client-core`](crates/finch-client-core) | Ядро клиента: QUIC-сессия, TOFU, REST-клиент |
| [`crates/finch-audio`](crates/finch-audio) | Звук: захват, шумоподавление, Opus, джиттер-буфер |
| [`crates/finch-screen`](crates/finch-screen) | Захват экрана, кодирование видео, звук демонстрации |
| [`crates/finch-protocol`](crates/finch-protocol) | Общие типы и сообщения протокола |
| [`crates/finch-testbed`](crates/finch-testbed) | Тестовый стенд: настоящий сервер, безголовые клиенты, метрики голоса |
| [`deploy`](deploy) | Docker-образ и `docker-compose.yml` для Linux |

## Сборка из исходников

Нужны Rust (stable), Node.js 22 и Windows C++ Build Tools. Нативное аудио (WebRTC) готовится скриптом `scripts/prepare-webrtc.ps1` — подробности в [NATIVE-AUDIO.md](docs/NATIVE-AUDIO.md).

```powershell
./scripts/prepare-webrtc.ps1 -Configuration Release   # задаёт FINCH_WEBRTC_ROOT
cd app/frontend; npm ci; npm test; npm run build; cd ../..
cargo test --workspace -- --test-threads=1
```

Готовые сборки клиента и сервера собирает и подписывает CI при публикации тега `v*`.

## Лицензия

[MIT](LICENSE). Сторонние компоненты — в [THIRD-PARTY-NOTICES.md](docs/THIRD-PARTY-NOTICES.md).
