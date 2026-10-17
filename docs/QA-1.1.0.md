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
| Полный Rust suite,30min soak,NSIS install/upgrade,signing/publication | PENDING |

Объективные сигналы синтезированы и не являются MOS. AGC не приписан выигрыш шумоподавления: gain-calibrated residual SNR использует одинаковые voiced windows и фиксированный RNNoise lag480samples; on/off отличаются≤0.03dB на stationary-noise fixtures. Уже clipped исходный сигнал учтён отдельно и не считается восстановленным. Полные таблицы и воспроизводимые команды: [BASELINE](../crates/finch-testbed/BASELINE.md).

## Найденные и исправленные регрессии
- FFT downsampling мог выйти за нормализованный диапазон и вызвать Earshot panic; ограничена только копия для анализа, DSP PCM не изменён ради теста.
- Native runtime queue silently dropped массовые изменения gain; bridge проверяет PostRuntimeSetting status, Rust передаёт только последнее изменение перед кадром.
- Одного очищения pre-roll было недостаточно: RNNoise/resampler хранили private audio. При reopening очищаются AEC/NS/VAD/resampler и device queue; exact private-vs-zero Opus regression проверен также после предшествующей публичной речи.
- Настройки фронтенда сериализованы, чтобы быстрые toggle/gain изменения не затирали подтверждённые значения. Native failure останавливает capture и сообщает UI, без raw bypass.

## Нативный клиент
Два release-клиента1.1.0 с раздельными WebView2/pin profiles вошли на сервер1.0.1 и в одну голосовую комнату. Проверены default-on/off/on, meter,3s microphone test, mute/unmute,deafen/undeafen,hear-myself warning. Остальные UI-сценарии и подтверждение итоговой сборки будут дополнены после завершения QA. Настройки/данные владельца не использовались; тестовые профили задаются FINCHSPEAK_PROFILE_DIR и WEBVIEW2_USER_DATA_FOLDER.

## Отдельная ручная приёмка владельцем
Два физических ПК/LAN/NAT; акустический feedback колонок/разные микрофоны; субъективная естественность голоса; физическое отключение/возвращение аудиоустройства. Эти пункты не объявляются PASS по одному локальному стенду.
