# Проверка микрофонов и мониторинга FinchSpeak — 19.10.2026

Начните с [итогов исследования](research-findings.md). Первопричина сбоя реального
устройства пока не доказана; воспроизведены механизмы подавления сигнала в
изолированных моделях. Это исследование диагностической 1.2.3 / d6275d2,
а не утверждение о поведении текущей main.

## Состав

- `report.md` — хронологический журнал с исправлениями субъективных оценок.
- `hs5-enhancement-on.json`, `hs5-enhancement-off.json`, `realtek-to-hs5.json` —
  реальные диагностические отчёты, `contains_audio=false`.
- `*-deltas.csv` — изменения накопительных счётчиков соседних снимков.
- `system-inventory.json` — сведения ОС, CPU и драйверов.
- `probe_aec.py`, `probe_clock.py` — временные исследовательские скрипты.
- `probe-results-summary.json`, `probe-controls-summary.json` — компактные результаты.
- `probe-results.json.gz`, `probe-controls.json.gz` — полные временные ряды уровней;
  gzip содержит исходные JSON без изменений. Это численные уровни, не аудиозаписи.
- `probe-clock-results.json` — результаты модели временных меток.

Установщик, DLL, скопированные исходники и данные аккаунта не включены.
Исходники исследуемой версии: [d6275d2](https://github.com/qwers451/FinchSpeak/tree/d6275d2).

## Воспроизведение

Используйте Python 3.13 на Windows x64, DLL из диагностической 1.2.3 и речевые
фикстуры коммита d6275d2. Скрипт ищет папку `crates/finch-testbed/fixtures`
относительно корня репозитория. Новые результаты пишутся рядом со скриптом.

```powershell
python docs/investigations/2026-10-19-audio/probe_aec.py --dll '<path-to-1.2.3>/finch_webrtc.dll'
python docs/investigations/2026-10-19-audio/probe_aec.py --dll '<path-to-1.2.3>/finch_webrtc.dll' --controls --output probe-controls.json
python docs/investigations/2026-10-19-audio/probe_clock.py
```

Условия, метрики, ограничения и SHA256 DLL приведены в итоговом отчёте.
Скрипты не открывают микрофон и не создают файлы с PCM.
