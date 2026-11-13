# QA 1.7.0

## Стиль и вёрстка (проход по коду)

Проход по коду в `app/frontend/src` (Task 13, часть «код»). Визуальная проверка снимками — отдельная задача, здесь не выполнялась: вёрстка проверена только тестами стилей (`src/styles/*.test.ts`), не в приложении при 900×600, 1280×800 и 1920×1080.

### Цвета, заданные не через переменные темы

| Файл | Было | Исправлено |
| --- | --- | --- |
| `styles/app.css` (21 место) | `rgba(255,194,14,…)`, `rgba(255,51,0,…)`, `rgba(0,180,255,…)` и т. п. для подсветки, плашек, баннеров, тегов, `#0A0F1C`, `#00B4FF` | `color-mix(in srgb, var(--token) N%, transparent)` и `var(--token)`: значения те же в Claude Dark, в «Своя» и Finch Custom берутся из палитры темы |
| `styles/finch.css` (31 место, правила после блока `:root`) | то же для тегов, баннеров, кнопок, аватаров, `.anno`, `.vstrip`, `.msg.mention`, `.spin` | то же |
| `styles/polish.css`, `motion.css`, `nest.css`, `music.css`, `soundboard.css`, `voiceNote.css`, `spatial.css`, `user-theme.css` | литералов с токенами не найдено | не менялось |

Тест `uses the theme token for tints…` в `stylePass.test.ts` следит, что литеральные `rgba()` от токенов не вернулись в файлы стилей.

### Инлайновые `style={{…}}` с цветом, размером и положением

| Файл | Было | Исправлено |
| --- | --- | --- |
| `components/VoicePanel.tsx` | цвет пинга `color`/`border-color` инлайном (`var(--teal)`/`var(--amber)` из JS); `font-size: 18px` у заголовка | `pingLabel` отдаёт `tone: 'good' \| 'slow'`, классы `.pill.ping-good/.ping-slow`; класс `.room-title` |
| `components/chat/Composer.tsx` | `color: var(--amber)` у «Upload failed»; `padding` ряда вложений; `max-width: 180px` у имени файла; `padding` подписи упоминаний | классы `.upload-failed`, `.attach-row`, `.attach-name`, `.mention-label` |
| `components/tree/TreeView.tsx` | `color: var(--red-text)` у «Вы делитесь экраном»; `min-width`/`text-align` у подписей | классы `.sharing-note`, `.tree-label`, `.tree-voice { min-width:0 }` |
| `components/voice/Dock.tsx` | `background` точки «говорит» инлайном | класс `.dot.talk` |
| `components/voice/LiveCard.tsx` | `background`, `width`, `height` точки в теге «Live»; `min-width` колонки | класс `.tag-dot`, `.live-titles` |
| `components/settings/SettingsModal.tsx` | `background-color` из палитры предложенной темы (6 образцов); `margin-right` точки; ширины полей 150 и 180 px (×3); `margin-top` подписей (×2); `font-size` отпечатка | переменная `--swatch-color` (как в `ThemeEditor`), классы `.chips i`, `.dot-gap`, `.field-narrow`, `.field-slider`, `.gap-top-8/16`, `.fp-value` |
| `components/settings/ThemeEditor.tsx` | `background-color` образца палитры пресета | переменная `--swatch-color`; правило `.chips i` в `user-theme.css` |
| `components/voice/SpatialStage.tsx`, `VoiceNote.tsx` (`height`), `VoicePanel` (`left`/`top` мест, `--rows`) | — | Оставлено: динамические значения по данным (позиция места, уровень, сетка), это допустимо по `UI-STYLES.md` |

Инлайн-размеры в файлах, которых этот проход не касался (`AdminModal`, `AuthModal`, `ChatArea`, `Overlays`, `MessageItem`, `AttachmentView`, `SearchBox`, `EmojiPicker`, `SharerPill`, `TofuModal`, `AvatarCropDialog`): в основном `min-width: 0`, `margin: 0` у `.link-btn` и размеры скелетонов. Цветов там нет. Не исправлено, отдельная задача.

### Нативные контролы

| Файл | Было | Исправлено |
| --- | --- | --- |
| `components/voice/SoundboardPanel.tsx` | `<input type="checkbox">` «Не проигрывать саундборд мне» без классов приложения | `Toggle` из `ui` с видимой подписью |
| `components/settings/ThemeEditor.tsx` | `<input type="color">` | Оставлено: нативный выбор цвета без аналога в `ui`; перекрыт образец `.theme-color .swatch input` в `user-theme.css` |
| `<select>` в `components/**` | не найдено | — |

### Карточки без среза углов в Finch Custom

В список `[data-theme="custom"] :is(…)` в `themes.css` (правила фона, рамки, `::before`/`::after` и `position`) добавлены: `.music-panel`, `.soundboard-panel`, `.soundboard-tile`, `.health-tile`, `.voice-note`, `.flock-card`, `.overlay-card`. `.ctx-menu` и `.select-listbox` уже были в списке. Для `.music-panel` внутри диалога узкого окна (ниже 940 px) псевдоэлементы отключены: диалог сам рисует рамку.

### Наложения и вёрстка

| Экран / место | Что было | Исправлено |
| --- | --- | --- |
| Комната, заголовок (`.mhead`) при открытой панели музыки | Перенос заголовка (`@container max-width:620px`) мерился по всей комнате (`.voice-room`), а не по колонке рядом с панелью. Когда панель открыта, колонка уже, и заголовок не переносился: название, кнопка «Позвать всех за собой», плашка музыки и пинг лезли друг на друга | `container-type: inline-size` перенесён на `.room-column` (`polish.css`) |
| Комната при 940–1150 px с панелью музыки | Сайдбар 232 + участники 200 + панель музыки 340 оставляли комнате около 170 px: плитки и док сжимались в ноль | В `music.css` при ≤1150 px панель музыки 280 px, а список участников скрыт, пока панель открыта. Ниже 940 px панель уже была диалогом |
| «Позвать за собой» (`.flock-stack`) | `bottom: 16px`: карточки ложились на док при узком окне | `bottom: 96px` (`app.css`); `.flock-text` с `overflow-wrap: anywhere` |
| Кнопка «Позвать всех за собой» и плашка музыки в заголовке комнаты | Могли сжиматься и обрезаться без многоточия | `.mhead > .btn { flex:none }`, `.music-now-chip { flex: 0 1 auto }` (`polish.css`) |
| Плитки здоровья (`.health-grid`) | `minmax(176px, 1fr)`: в узком контейнере ширина не уменьшалась | `minmax(min(176px, 100%), 1fr)` |
| Саундборд (`.soundboard-grid`, `.soundboard-add`) | Минимум 120 px у плитки и 90 px у кнопки/поля в колонке 420 px диалога | `min(120px, 100%)`, `minmax(0, 1fr) 90px` |
| Голосовое сообщение (`.voice-note`) | `max-width: 340px` без учёта ширины пузыря | `max-width: min(340px, 100%)` |
| Гнёздо (`.nest`) при наведении | Панель действий (`.acts`) встаёт поверх названия гнезда | при наведении и фокусе `.nest-top` получает `padding-right: 44px` (`nest.css`) |

### Что не исправлено и почему

- Нейтральные тени и вуали (`rgba(0,0,0,…)`, `rgba(5,8,15,…)`, `#fff` у ползунка), чёрный фон видео (`#05080F`) и подложка полосы громкости (`#0B1020`): не зависят от темы, оставлены.
- Декоративный градиент заглушки демонстрации (`.live-art`, `#1B2A55`→`#0F7A4D`) и цвет текста «Информация» (`#8DB8FF`): художественные значения, оставлены.
- Цвета наведения `#2D3858`, `#3A4770`, `#5446FF`, `#FF5A33` в `finch.css`: в Finch Custom и «Своя» перекрываются правилами `themes.css`/`user-theme.css`; в Claude Dark выглядят как раньше. Перевод на `color-mix` отложен.
- Блок `:root` в `finch.css` (палитра) не трогался: это источник токенов.
- Проверка в приложении (900×600, 1280×800, 1920×1080, большой текст, русский язык, Finch Custom) не выполнялась: это задача визуального прохода.
