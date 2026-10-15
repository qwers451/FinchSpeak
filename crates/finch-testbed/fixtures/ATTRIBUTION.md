# Speech fixtures

Synthesized locally with the Windows Speech API (`System.Speech.Synthesis`),
16 kHz, 16-bit, mono, trimmed to at most 5 s. Generated output, no third-party
recordings; no attribution or license obligations.

| File | Voice | Text |
|---|---|---|
| `speech_1.wav` | Microsoft Irina Desktop (ru-RU) | «Привет! Проверка связи. Как меня слышно? Давай обсудим планы на вечер.» |
| `speech_2.wav` | Microsoft Zira Desktop (en-US) | "Hello everyone, this is a voice quality test for the echo canceller." |
| `speech_3.wav` | Microsoft Irina Desktop (ru-RU) | «Я говорю поверх эха, и мой голос должен остаться разборчивым.» |

Regenerate with `SpeechSynthesizer.SetOutputToWaveFile(path, SpeechAudioFormatInfo(16000, Sixteen, Mono))`,
`Rate = 1`, then trim to 5 s.
