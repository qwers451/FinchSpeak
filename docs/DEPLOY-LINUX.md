# Сервер FinchSpeak на Linux (Docker)

Сервер работает в контейнере от непривилегированного пользователя. База, сертификат (его отпечаток закрепляют клиенты), вложения, резервные копии и журналы хранятся в Docker-томе `finch-data` и переживают перезапуски и обновления. В работе сервер занимает 10–60 МБ памяти; `docker-compose.yml` ограничивает его 512 МБ.

Нужны Docker с плагином Compose (`curl -fsSL https://get.docker.com | sh`), публичный IP и открытые порты TCP 8443 и UDP 4433.

## Вариант 1. Собрать на сервере

Просто, но компиляция Rust занимает 20–40 минут на одном ядре и требует около 2 ГБ памяти (на маленьком VPS добавьте временный swap).

```bash
git clone --depth 1 https://github.com/qwers451/FinchSpeak.git
cd FinchSpeak/deploy
docker compose up -d --build
```

## Вариант 2. Собрать на другом компьютере и перенести (рекомендуется для маленького VPS)

На любом компьютере с Docker (Windows, macOS, Linux; для сервера x86_64):

```bash
git clone --depth 1 https://github.com/qwers451/FinchSpeak.git && cd FinchSpeak
docker build -f deploy/Dockerfile -t finchspeak-server:latest .
docker save finchspeak-server:latest | gzip > finchspeak-server.tar.gz
scp finchspeak-server.tar.gz deploy/docker-compose.yml deploy/finch-server.toml user@SERVER:~/finchspeak/
```

На сервере:

```bash
cd ~/finchspeak
docker load < finchspeak-server.tar.gz
docker compose up -d --no-build
```

## Порты и файрвол

Проверьте, что порты свободны:

```bash
sudo ss -tulpn | grep -E ':8443|:4433'
```

Если их занимает другая программа, в `docker-compose.yml` поменяйте **левую** часть, например `"9443:8443/tcp"` и `"5433:4433/udp"` — тогда участники подключаются к `https://SERVER:9443`.

Откройте порты, только если файрвол уже включён (не включайте ufw впервые на удалённом сервере — можно потерять SSH):

```bash
sudo ufw allow 8443/tcp
sudo ufw allow 4433/udp
```

У облачных провайдеров порты открываются ещё и в панели (Security group / Firewall).

## Проверка и первый вход

```bash
docker compose ps                    # через ~20 с статус "healthy"
curl -ks https://127.0.0.1:8443/health
docker compose logs | grep fingerprint
```

Строку `TLS certificate fingerprint (SHA-256): …` передайте участникам — клиент попросит сверить её при первом подключении. Первый зарегистрированный аккаунт становится администратором.

## Настройки

`finch-server.toml` монтируется в контейнер только для чтения; после изменения выполните `docker compose restart`. Полезное:

| Ключ | По умолчанию | Зачем менять |
| --- | --- | --- |
| `[auth] token_expiry_hours` | `720` (30 дней) | Сколько живёт сессия без использования; на старых серверах было 72 |
| `[auth] registration` | `"invite"` | `"open"` — регистрация без приглашений |
| `[storage] max_file_size_mb` | `50` | Меньше — экономия диска |
| `[backup] keep` | `7` | Сколько суточных копий базы хранить |

## Обслуживание

- Журналы: `docker compose logs -f` (Docker хранит до 30 МБ, см. `logging` в compose); журналы сервера — в томе, `/app/data/logs`.
- Резервные копии базы — автоматически раз в сутки в `/app/data/backups` внутри тома. Где том на диске: `docker volume inspect <проект>_finch-data`.
- Обновление: загрузите или соберите новый образ, затем `docker compose up -d`. Данные и отпечаток сохраняются.
- **Не удаляйте том `finch-data`** и не выполняйте `docker volume prune` — пропадут база и сертификат, и всем участникам придётся заново подтверждать сервер.
