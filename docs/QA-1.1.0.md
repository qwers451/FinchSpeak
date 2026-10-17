# QA FinchSpeak1.1.0

Статус: кандидат; независимое ревью завершено, P1 исправлен, полный повторный набор проверок зелёный; публикация ещё не завершена. Все результаты относятся к Windows x64 на одном ПК; два физических ПК, NAT и прослушивание человеком не имитировались.

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
| Полный Rust workspace suite | 408 PASS,0fail;2 ignored (load50 и physical microphone probe),оба отдельно PASS |
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


## Итоговое независимое ревью и исправление P1
Один независимый reviewer проверил всю ветку a973bfc..c502b82, спецификацию, план и измерения. Найден один P1: очистка consumer-очереди не отсекает остаток уже работающего callback, старый аппаратный буфер и быстрое закрытие/открытие между опросами worker. Два новых теста сначала упали (p1-red.log), затем прошли с исправлением.

Теперь mute/PTT/mic-test публикуют controls и монотонное поколение одной атомарной операцией. Callback фиксирует поколение один раз, не переименовывает старые сэмплы и проверяет capture/callback timestamps в часах устройства. Первый callback нового поколения отбрасывается; второй задаёт временную границу и тоже отбрасывается; последующие блоки разрешены только если целиком записаны после границы. Неверные/непродвигающиеся часы закрывают передачу. Worker отвергает смешанные/старые поколения, очищает FFT/AEC/NS/VAD/pre-roll перед первым публичным блоком и проверяет поколение перед отправкой. Локальный mic-test/monitor продолжает получать private PCM. Ни locks, ни аллокации, ни DSP в callback не добавлены; метаданные увеличивают capture ring примерно на192KiB.

Регрессии проверяют детерминированный in-flight producer, поздний hardware backlog, rapid mute/PTT/test transitions, mixed partial input на48/44.1kHz и identifiable private marker через настоящий VoiceCapture/Opus (on/off, с предшествующей публичной речью и без), сравнивая пакеты с zero-input control. Дополнительная явная проверка настоящего WASAPI микрофона: три закрытия/открытия, разрешённые блоки после каждого открытия, ни одного во время закрытого состояния, ошибок устройства0 — PASS. Это проверка provenance/часов устройства, не субъективного звучания.

Цена защиты: после открытия теряются первые два callback и весь непроверенный аппаратный backlog; длинные буферы устройства могут задержать начало передачи. Новая постоянная очередь не добавляется; задержка установившегося тракта измеряется отдельно. Gain/toggle не меняют privacy generation и не сбрасывают AEC.

### Решения по замечаниям и пределам ревью
P1 исправляется до публикации; других важных или мелких code findings нет. Все12 решений ledger приняты reviewer, кроме неполной producer-границы Task4, дополненной описанным исправлением:
1. Standalone original APM/Meson вместо полного GN; риск ошибки ABI/сборки проверен native probes.
2. MSVC cl вместо clang-cl после реального SIMD build failure; риск SIMD/runtime проверен x64 probes.
3. Exact source/tools pins + записанный host MSVC/SDK вместо требования локального Mozilla toolchain на CI; риск неверного cache/ABI ограничен fingerprint/hash validation.
4. C++20 последовательно для всех native subprojects; риск несовместимой сборки ограничен одним standard/CRT.
5. Synthetic AEC assertions после2s адаптации; риск более долгого стартового эха остаётся в акустической приёмке, post-convergence gates не ослаблены.
6. Старый unreleased compressor assertion заменён утверждёнными AGC2 quiet/spread/loud gates; риск слабого выравнивания проверен на трёх голосах.
7. Privacy opening сбрасывает delayed AEC/NS/VAD/resampler, сохраняя gain adaptation; producer/timestamp gap закрыт P1. Риск — повторная адаптация AEC и onset loss, нужны реальные колонки.
8. Opus silence сравнивается с codec zero-control; риск скрытого codec-state leak проверен также после public/private sequences.
9. Невалидные gain-calibrated windows возвращают Result; риск неверного caller handling проверен metric tests.
10. Исторические NS gates сохранены в AGC-off controls, плюс full enabled A/B; риск маскировки gain проверен identical windows/fixed lag.
11. Version/resource staging выполнен до GUI QA, чтобы проверять настоящий1.1.0; риск stale payload ограничен final artifact hashes.
12. Независимые package/install builds шли параллельно soak; риск влияния нагрузки отражён числом поздних deadlines, а не выдан за dropout-free physical PASS.

Не объявляются PASS без подходящего стенда: subjective naturalness, реальные колонки/микрофоны и AEC reconvergence, два физических ПК/LAN/NAT/unplug; acoustic recapture хвоста mic-test (done означает постановку последнего кадра в output queue, а не гарантированную тишину колонок); назначение глобальной PTT-клавиши и явный выбор устройства в native UI; полный updater confirmation/download/installer/relaunch UI из1.0.1 и сохранение реальных старых voice preferences (проверенный isolated marker — более узкое доказательство); dropout-free physical capture по headless soak. Эти пределы сохраняют OWNER CHECK, а не ослабляют автоматические quality/privacy gates. Облачные проверки, stable publication и свежие публичные assets должны получить отдельные реальные результаты ниже.

Итог исправления P1: p1-red.log —2 FAIL до реализации; p1-green.log —34 app tests PASS; p1-device-final.log —1 explicit physical probe PASS; p1-workspace.log —408 PASS/0FAIL/2ignored,64suite results; p1-check.log и p1-clippy.log all-targets PASS; frontend271 PASS/build PASS. Второе ревью не запускалось, один предусмотренный планом fix pass закрыт регрессиями и полным набором тестов.

После P1 выполнена новая Release/NSIS сборка из7a52cb4: fresh install/upgrade1.0.1/uninstall, ресурсы и recursive PE closure PASS. Installed EXE SHA256 FC7D9B0F589E9595934BD9230D3483B26E4A048D0A3389162C12E74649BCD6E1; native DLL SHA256341702EDCB6686C0D19DF96625E4C509A594A68319E19EB08668A0DA7297DA84. Оба заново упакованных ZIP: actual BAT/error cases/API/invite/backup/restore PASS; portable client из ZIP запускается с system PATH/foreign cwd/изолированным профилем PASS. Новая подпись реального NSIS: genuine accepted, modified installer rejected, version/URL/old/equal/new offer PASS. Local installer SHA256937a283eb9aa365406e73cdc8c70862804a4c552476585973de4580076385ad1.

Облачный preflight39033798802 наc502b82 уже прошёл original native build, frontend, clippy, полный Rust suite, NSIS и серверный ZIP/API smoke; signing/install ещё выполняются. Он проверяет поставку до P1, поэтому исправление дополнительно проверяется обязательным CI основной ветки и окончательным tag workflow до публикации. Ошибка SQLite предыдущего preflight39018059366 не повторилась; production timeout/test gates не менялись.
