# QA FinchSpeak1.1.0

Статус: кандидат, публикация и итоговое независимое ревью ещё не завершены. Все результаты относятся к Windows x64 на одном ПК; два физических ПК, NAT и прослушивание человеком не имитировались.

## Звук
Оригинальный C++ WebRTC APM M131/standalone2.1, AEC3 и AGC2; RNNoise/nnnoiseless0.5.2 и Earshot1.2.2 сохранены. Свои compressor/DSP и Sonora удалены. Порядок: AEC → RNNoise → независимая pre-AGC оценка порога → AGC2/limiter → Opus. Ручное усиление передаётся оригинальному gain controller, нормализованный вход не превышает±1. Enabled ceiling−1dBFS, переключение20ms, без новой очереди кадров.

| Проверка | Результат |
|---|---|
| Native Debug+Release:200 create/reset/drop,40000blocks,allocation/status validation | PASS |
| Audio unit tests после регрессий | 78 PASS |
| A/B:3 голоса ×3 уровня,15s,общие последние5s после10s адаптации | 5 tests PASS; тихая речь+10.95…11dB, разброс сокращён>12dB |
| NS без AGC: white/pink5dB input SNR | +22.2/+15.7dB, segmental SNR сохранён |
| AEC без/с AGC после адаптации | ERLE74.4/75.4dB, double-talk loss2.5/3.5dB |
| Паузы pink/white: первый/последний полный1s | −66.14/−92.71 и−73.35/−95.58dBFS; pumping нет |
| Headless localhost latency:3 последовательных запуска | 76.4/76.4/76.4ms, без физических буферов устройств |
| Netem5% loss,0…60ms jitter | probes116.44/96.42/96.44ms; maxbuffer5,zero-run0,268/284 packets |
| Transport load50clients | voice/video/chat100%,0disconnects; chatp95=64ms; это не50 DSP одновременно |
| Frontend suite | 271 tests/28files PASS |
| Обратная совместимость с опубликованным1.0.1 | SHA256 verified; pinned TLS→admin/invite/member→chat→двусторонний native Opus, on/off PASS |
| Свежий сервер1.1.0 | Те же сценарии PASS,144/144packets в каждом направлении/режиме,maxbuffer5 |
| PE closure свежего клиента и APM DLL | PASS, только системные Windows imports; native DLL static CRT |
| Локальный NSIS: fresh1.1.0 /published1.0.1→1.1.0 /uninstall | PASS; installed EXE/DLL hashes match fresh; isolated marker preserved |
| Оба ZIP:actual BAT/Cyrillic/spaces/foreigncwd/error cases;API/invite/backup/restore | PASS |
| Полный Rust workspace suite | 404 PASS,0fail;1load test ignored в общем запуске,отдельно PASS |
| cargo check/clippy all-targets, -Dwarnings | PASS |
|30min real-time production capture soak |90000frames,18000privateframes,59recreations,0processingerrors; bounded output |
| Private working set на5й/30ймин |3497984→3715072bytes,+217088bytes(0.21MiB),порог20MiB PASS |
| Soak DSP/scheduling diagnostics |p95=7.91ms,max=45.162ms;268late20msdeadlines под параллельными сборками,это не zero-deadline или physical-device guarantee |
| Локальная подпись/manifest/tamper и update offer |PASS: встроенный ключ,genuine accepted,tamper rejected;1.0.1offer,равный/новый неoffer |
| Облачный Release candidate /public stable assets |PENDING |

Объективные сигналы синтезированы и не являются MOS. AGC не приписан выигрыш шумоподавления: gain-calibrated residual SNR использует одинаковые voiced windows и фиксированный RNNoise lag480samples; on/off отличаются≤0.03dB на stationary-noise fixtures. Уже clipped исходный сигнал учтён отдельно и не считается восстановленным. Полные таблицы и воспроизводимые команды: [BASELINE](../crates/finch-testbed/BASELINE.md).

## Найденные и исправленные регрессии
- FFT downsampling мог выйти за нормализованный диапазон и вызвать Earshot panic; ограничена только копия для анализа, DSP PCM не изменён ради теста.
- Native runtime queue silently dropped массовые изменения gain; bridge проверяет PostRuntimeSetting status, Rust передаёт только последнее изменение перед кадром.
- Одного очищения pre-roll было недостаточно: RNNoise/resampler хранили private audio. При reopening очищаются AEC/NS/VAD/resampler и device queue; exact private-vs-zero Opus regression проверен также после предшествующей публичной речи.
- Настройки фронтенда сериализованы, чтобы быстрые toggle/gain изменения не затирали подтверждённые значения. Native failure останавливает capture и сообщает UI, без raw bypass.

## Нативный клиент
Два release-клиента1.1.0 с раздельными WebView2/pin profiles вошли на сервер1.0.1 и в одну голосовую комнату. Проверены default-on/off/on, meter,3s microphone test, mute/unmute,deafen/undeafen,hear-myself warning. Повторный вход в голос PASS; демонстрация собственного тестового окна со звуком включилась, второй клиент декодировал1280×800/30FPS, просмотр/демонстрация остановлены. Скриншот и accessibility tree сохранены под target/voice-enhancement-ui в основном checkout. Назначение глобальной клавиши и именованного устройства не объявлены PASS: keyboard delivery не подтвердился, popup устройства оказался отдельным окном WebView2, затем системное окно Windows Security перекрыло клиент. Автоматизация системных разрешений не выполнялась; эти пункты требуют ручной проверки. Настройки/данные владельца не использовались; тестовые профили задаются FINCHSPEAK_PROFILE_DIR и WEBVIEW2_USER_DATA_FOLDER.

## Отдельная ручная приёмка владельцем
Два физических ПК/LAN/NAT; акустический feedback колонок/разные микрофоны; субъективная естественность голоса; физическое отключение/возвращение аудиоустройства. Эти пункты не объявляются PASS по одному локальному стенду.
