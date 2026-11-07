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
  <a href="docs/RELEASE-CHECKLIST.md">Проверка на двух ПК</a>
</p>

---

## Почему FinchSpeak

- 🏠 **Самохостинг** — сервер ваш, история, файлы и аккаунты остаются у вас.
- 🎙️ **Чистый голос** — шумоподавление WebRTC и RNNoise, Opus; сервер пересылает звук без перекодирования.
- 🖥️ **Демонстрация экрана со звуком** — зрители слышат игру или видео, но не самих себя.
- 🔐 **Вход сохраняется** в диспетчере учётных данных Windows, автозапуск и работа из трея.
- 🔗 **Приглашения ссылкой** `finchspeak://join` — сервер, код и отпечаток сертификата в одной ссылке.
- 💬 **Текстовые каналы** с ответами, реакциями, упоминаниями, поиском и вложениями.
- 🎨 Две темы (**Claude Dark** и **Finch Custom**), русский и английский интерфейс.

## Быстрый старт

**Участнику** — установите `FinchSpeak_<версия>_x64-setup.exe` из [Releases](https://github.com/qwers451/FinchSpeak/releases/latest) и подключитесь по ссылке-приглашению или адресу `https://АДРЕС:8443`.

**Владельцу сервера (Windows)**

1. Распакуйте `FinchSpeak-server-windows.zip` и запустите `Run_Server.bat`.
2. Откройте порты **TCP 8443** и **UDP 4433**.
3. При первом подключении сверьте отпечаток сертификата с окном сервера.
4. Первый зарегистрированный аккаунт становится администратором; остальных зовите через **Администрирование → Приглашения**.

**Владельцу сервера (Linux, Docker)** — образ и `docker-compose.yml` лежат в [`deploy/`](deploy):

```bash
cd deploy
docker compose up -d --build
```

## Требования к серверу

Хватит одного ядра и нескольких десятков мегабайт памяти. Главное — исходящий канал: голос ≈ 0,1 Мбит/с на слушателя, экран 1080p30 ≈ 5 Мбит/с на зрителя.

## Как устроено

```
Клиент (Tauri 2 + SolidJS)  ──HTTPS :8443 / QUIC :4433──►  Сервер (Rust SFU + SQLite)
```

Код клиента — в [`app`](app), сервер, аудио, демонстрация экрана и протокол — в [`crates`](crates).

## Сборка

Нужны Rust (stable), Node.js 22 и Windows C++ Build Tools. Нативное аудио готовится скриптом `scripts/prepare-webrtc.ps1` — подробности в [NATIVE-AUDIO.md](docs/NATIVE-AUDIO.md).

```powershell
cd app/frontend; npm ci; npm test; npm run build
cargo test --workspace -- --test-threads=1
```

## Безопасность

Соединение защищено TLS 1.3, сертификат сервера закрепляется при первом подключении (TOFU). Сквозного шифрования нет — сервер принадлежит его администратору.

## Лицензия

[MIT](LICENSE). Сторонние компоненты — в [THIRD-PARTY-NOTICES.md](docs/THIRD-PARTY-NOTICES.md).
