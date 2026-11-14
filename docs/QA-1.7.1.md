# QA 1.7.1

## Root causes

- A1 (sound set choice does not apply in the settings modal): the z-index token `--z-popover` is declared only under `[data-theme="custom"]`, so in the default `claude` theme `.floating-panel` (the Select list, portalled to body) has `z-index: auto` and paints under `.overlay` (`z-index: 50`); clicks on the options land on the overlay, which closes the settings, instead of choosing the sound set. Fixed by declaring the depth scale at `:root` in `app.css`.
- A2 (Appearance page clipped on the left): only in «Мои цвета», the swatch's hidden colour input (`.theme-color .swatch input`, `inset:-8px`) and its focus ring overhang the swatch, which sits at the left edge of `.settings-page` (`overflow:auto`, `padding:0 4px 0 0` in `themes.css`), so they are clipped; Claude Dark, Снегирь and Finch Custom at 900×600 and 1280×800 show nothing clipped. Fixed by giving `.settings-page` 8px of left padding.
- A3 (music panel looked unstyled): MusicPanel.tsx uses `tabular` and `music-queue-block`, which have no rule in music.css or global CSS (`tabular` is defined nowhere), and the now-playing block, tabs and empty state had no shared-style treatment; music.css is imported in index.tsx and the other panel classes match. Fixed by card styling for `.music-now`, `.music-clock`, `.music-queue-block`, shared `.tabs`/`.tab` and a FinchBird empty state.
- A4 (sent messages look "crooked"): the echo replaces `local-<client_msg_id>` with the server id, so `ChatArea` (keyed by `message.id`) unmounted the pending row and mounted a new one that replayed `msg-enter`; additionally `msg-enter` was set via `classList` beside a dynamic `class` string, so the `sending` to stored change stripped it mid-animation. Fixed by keying rows by `client_msg_id` and putting `msg-enter` in the class string. Scroll-to-bottom after own send was already correct (transforms do not change scrollHeight).
- A6 (voice messages do not send): the mic button leaves the DOM as soon as recording starts (`Show` swaps it for `RecordingBar`), so the pointer release was handled by the removed button and never reached the recorder; the recording ran until the two-minute limit or a cancel. Fixed by following the press on `window` (`pointerup`, `pointermove`, `pointercancel`). The server, protocol and upload path accept an empty text with an audio/ogg attachment (checked in code, not run).

## Animations

Audit in code (no client run, no frame capture: the controller takes the captures). Verdicts: good (no cause found), fixed (cause found and fixed with a regression test), removed (rule dropped, instant behaviour). Tests: `ui/motion.test.ts`, `styles/motionLive.test.ts`.

| Animation | Cause found / none | Verdict |
| --- | --- | --- |
| 1. Chat: new message, own message, wave | none in code now (A4 fixed the replay on echo); stagger capped by `--i` | good |
| 2. Reactions: pop | none; transform only, skipped under reduce motion | good |
| 3. Badge counter: pop | none; grows only, does not pop on shrink | good |
| 4. Rolling numbers and text | none significant; a new change adds a WAAPI on top of a running one, the newest wins | good |
| 5. Voice room: joiner grows in | a tile already growing in (220 ms) that another join moved was glided too, so its WAAPI translate replaced the CSS scale; fixed: a tile with a running CSS animation is neither glided nor snapshotted | fixed |
| 6. Voice room: leaver shrinks out | `createPresence` appended a leaving entry at the end of the list, so the tile jumped to the last cell and shrank there while the others moved past it; fixed: a leaving entry keeps its slot until the exit ends | fixed |
| 7. Tile rearrangement (FLIP, `animateLayout`) | (a) positions from the last call were absolute rects: after a window or room resize, or a scroll, the next reorder glided tiles that had not moved; (b) a glide interrupted by another reorder stacked on the first one and jumped; fixed: positions relative to the room, no glide after a resize (window or room width), an interrupted glide continues from its visible place | fixed |
| 8. Theme reveal (circle from the click) | none in code: clip-path on the new snapshot only, instant without the API or under reduce motion; not checked in WebView2 | good |
| 9. Dock icons: turn and spring | overshoot on release is about 0.3% of scale (0.96 to 1), not visible; icon turn replays only when the icon changes (`For` keyed by icon) | good |
| 10. Buttons: hover lift, press | none; transform only, hover lift only with a fine pointer, off under reduce motion | good |
| 11. Toggle and slider thumb | none; colour changes at once, thumb moves by transform | good |
| 12. Speaking indicator (ring, waves, pulse, bars, glow) | none in code; box-shadow is static, animated properties are transform and opacity | good |
| 13. Modal, toast, menus (exit and enter) | none; exit times match the 140 ms hold; toast spring overshoots about 1 px and does not snap | good |
| 14. Call card and its strip (30 s) | none; the strip is scaleX with a linear 30 s timer, no layout property | good |
| 15. Soundboard: ring and name bubble | none; the name bubble is keyed, so it replays for every sound from the same person | good |
| 16. Bird mascot: sway, blink, waves | none; off under the app setting and the system preference | good |
| 17. Finch nest: sway and hop | sway removed mid-way on mouse-out snaps back by at most 1.2 degrees; accepted as too small to justify a rule | good |
| 18. Server health sparkline | none; `pathLength="1"` makes the dash offset draw the line, refresh does not replay it | good |
| 19. Loading skeletons | none; opacity only | good |
| 20. Spatial stage: seats move between places | seats transitioned `left` and `top` (layout properties, re-laid out each frame of the 260 ms move); fixed: the seat is placed with a `translate` (sideways in cqw of the stage, which is a size container), the transition is on `transform` only, a dragged seat has no transition so it follows the pointer, reduce motion (app setting and system) switches the transition off | fixed |

Counts: 16 good, 4 fixed (rows 5, 6, 7, 20), 0 removed.

Open concern (not changed): when the room has more people than tiles, the speaking set decides which tiles show, so a change of who is speaking reorders the grid and the tiles glide each time. This is by design of `arrange`; no frame capture was taken to judge how often it happens.

## Клики после анимаций

Audit in code and Vitest (happy-dom); the desktop client was not run. Verdicts as above. Tests: `ui/ContextMenu.test.tsx`, `ui/Select.test.tsx`, `ui/popover.test.ts`.

Causes found:
- (а) Scroll closed menus. `ContextMenuHost` and `Select` closed on any capturing `scroll` on the window, so a chat auto-scroll or smooth `scrollIntoView` (`ChatArea.tsx`) closed a menu the moment it opened. Fixed: a scroll closes only when it happens in the anchor, in its ancestors, or in the document (`scrollMovesAnchor`). A point anchor (right-click) takes the element under the point when the menu opens (`document.elementFromPoint`) and closes when that element or its ancestors scroll; with nothing under the point, only a document scroll closes it.
- (б) Submenu closed on the way to it. Every sibling entry's `pointerenter` closed the submenu at once. Fixed: the close waits 150 ms (`SUBMENU_CLOSE_DELAY`); entering the submenu panel or its trigger cancels it.
- (в) Placement measured the scaled panel. `placePopover` used `getBoundingClientRect` while the panel was in `@starting-style` (`scale(.98)`), so the size was about 2% small and a flipped panel overlapped its anchor by a few px. Fixed: the panel's size is `offsetWidth`/`offsetHeight`; the anchor stays `getBoundingClientRect`.
- (г) Same-click open and close: checked, not found. The `mousedown` outside listener is added after the opening click has been dispatched; the `click` on an entry closes the menu before `onSelect` runs (test: one call, menu closed).

| Element | What happens | Cause | Verdict |
|---|---|---|---|
| Participant menu (tile, sidebar, member row: `participantMenu.ts`) | closes when the chat or member list scrolls, including right after opening | (а) | fixed |
| «Переместить» submenu in the participant menu | closes when the pointer crosses a sibling entry on the way in | (б) | fixed |
| Submenu and menu placement near the right and bottom edges | 2% small size during the entrance; flipped panel overlaps the anchor by a few px | (в) | fixed |
| Flock picker (`VoicePanel.tsx`, `openFlockMenu`) | same as the participant menu | (а), (в) | fixed |
| Select listbox (`Select.tsx`; device and option pickers in the settings modals) | closed on any scroll, so a click could be lost after the chat moved | (а), (в) | fixed |
| Emoji picker, mention menu, search results (`EmojiPicker.tsx`, `Composer.tsx`, `SearchBox.tsx`) | no scroll listener, no `pointer-events` issue in code; entrance is opacity and transform only | none | good (not clicked in the client) |
| Channel, message, «Ещё» and server menus | no `openContextMenu` caller for them exists in the code (only the participant menu and the flock picker use it); nothing to test here | n/a | not found in code; check on the client |
| `.vc.vc-leave` (leaving tile, `motion.css:151`) | kept in DOM for the exit | `pointer-events:none` already set | good |
| `.overlay[data-exiting]` and the modal inside it (`Modal.tsx:72`, `motion.css:15-16`) | kept for 140 ms | `pointer-events:none` on the overlay during exit | good |
| `.toast[data-exiting]` (`ToastHost.tsx:51`, `motion.css:133`) | kept for the exit | `pointer-events:none` already set | good |
| `.drop-overlay` (`app.css:147`) | covers the chat while dragging | `pointer-events:none` already set | good |
| `.viewer-hud` (`app.css:187-188`) | hidden at opacity 0 | `pointer-events:none` unless `.on` | good |
| `.range-value`, `.ripple-dot`, `.btn-spinner`, `.sb-said`, `.jump-wrap` wrapper | decorative or hidden layers | `pointer-events:none` already set; `.jump-wrap .btn` re-enables clicks | good |
| `.popover-host` (portal target) | no box, no position, no size rules | panels are `position:fixed` and nothing covers the page | good |
| `createPresence` consumers: `Nest.tsx` (hold 0), `FlockCallCard.tsx` (hold 0), `VoicePanel.tsx` tiles (hold `MOTION.exit`) | an item is kept for one exit only | removed items are either gone at once or `vc-leave` with `pointer-events:none` | good |

Counts: 9 good, 5 fixed (participant menu, submenu, placement, flock picker, Select listbox), 1 not found in code.

Open concern (not changed): a submenu's anchor is still read with `getBoundingClientRect`. If the pointer reaches a submenu trigger within the 160 ms entrance of its parent, the anchor is about 2% off. The brief keeps the anchor rect; no client capture was taken.

## Визуальный аудит

Владелец: «не забудь проверить, где накладывается текст и формы и что везде всё стилизовано. Сейчас в панели музыки, например, есть наложения и нестилизованные части».

**Как проверяли.** Настоящая вёрстка без сервера и без Tauri: вход `?qa=<экран>&theme=&lang=ru` (`app/frontend/src/qa/`, только `npm run dev`: подключение стоит под `import.meta.env.DEV` и в сборку не попадает — проверено поиском по `dist/`, и тестом на `index.tsx`). Экраны строятся из настоящих компонентов с подставными данными: длинные русские имена, 12 участников, 50 треков, загрузка и ошибка загрузки. Скрипт `target/release-qa-v1.7.1/visual-audit.mjs` запускает headless Edge по CDP и на каждом экране проверяет: текст без многоточия, вылезающий из блока; текст, обрезанный предком; многострочные метки и теги; пересечение блоков одного слоя (`getBoundingClientRect` с учётом прокрутки); кнопки и поля с браузерным стилем (`appearance` у select, системный шрифт Arial/monospace, `outset`-рамка, серый фон кнопки); цвет текста по умолчанию (чёрный, ссылочный); обрезка с многоточием без `title`.

**Охват.** 19 экранов: текстовый канал, голосовая комната на 1, 4 и 12 участников, плеер (пустая очередь, длинные названия, загрузка, ошибка загрузки, библиотека на 50 треков, вкладка «С моего компьютера»), саундборд, режим Finch (блок «В комнате» в компактном виде), 5 страниц настроек, администрирование, контекстное меню (длинное и меню участника). Окна 1280×800, 900×600 (все четыре темы: claude, bullfinch, custom, user) и 1920×1080 (claude). 180 прогонов. Скриншоты: `target/release-qa-v1.7.1/visual/before/` и `visual/after/` (после — все прогоны), данные: `visual/before.json`, `visual/after.json`.

**Итог.** До: 1046 срабатываний в 25 группах. После: 0. Нарочно оставлено и исключено из проверки: стопка аватаров в гнезде (отрицательный отступ), рисунок гнезда, заходящий под название на 4 px, перенос подписи «Клавиши и о программе» в боковом меню настроек на две строки (перенос — выбранное поведение, `.setnav .si`).

| Экран | Проблема | Исправление | Тест |
| --- | --- | --- | --- |
| Левая панель, голосовые каналы, строка комнаты и список людей (все экраны с ≥2 в комнате) | Метка «В ЭФИРЕ» переносилась на две строки, налезала на название и счётчик | `.tag { white-space:nowrap; flex:none }` (`polish.css`) | `visualAudit.test.ts` «a tag never wraps…» |
| Режим Finch, гнездо с трансляцией | То же: метка на две строки, шапка гнезда заезжала на рисунок | то же правило | то же |
| Левая панель, шапка | Кнопка «Режим Finch» рядом с названием выходила на 23 px в соседнюю колонку (текстовый канал, голосовая комната) | кнопка переехала в свою строку под шапкой `.finch-switch` (`app.css`, `Sidebar.tsx`) | `visualAudit.test.ts` «Finch mode switch…», `Sidebar.test.tsx` |
| Режим Finch, нижняя полоса | При играющем треке название не сжималось, «Голос подключён», имя и кнопки накладывались друг на друга | компактные «Инструменты комнаты»: название усекается (макс. 220 px), часы скрыты, кнопки не сжимаются; полоса переносит элементы на вторую строку, статус голоса может сжаться (`roomTools.css`, `nest.css`) | `visualAudit.test.ts` «compact room tools…», «perch…» |
| Окно плеера, «Сейчас играет» | Пузырёк значения ползунка («31») лежал на строке «Исполнитель · Добавил…» | пузырёк у ползунка перемотки скрыт, позицию показывают часы рядом (`music.css`) | `visualAudit.test.ts` «music progress slider…» |
| Блок «В комнате» (A9, перенесено) | Кнопки «Пауза» / «Следующий» были только текстом, в узкой панели и в режиме Finch обрезались | значки `pause`, `resume`, `next` (в наборах classic и custom), подпись `rt-label` скрывается в компактном режиме и на узкой панели, имя и подсказка остаются | `RoomTools.test.tsx` «music row buttons…», `roomTools.test.ts`, `customIcons.test.tsx` |
| Администрирование, таблица участников | Заголовок «Последняя активность» выступал за ячейку на 2 px | `.members-table th { overflow-wrap:anywhere }` (`polish.css`) | `visualAudit.test.ts` «admin table header…» |
| Плеер: очередь, библиотека, строка «Сейчас играет», загрузки (в том числе ошибка) | Длинные исполнитель, «Добавил: …», имя файла и текст ошибки обрезались многоточием без подсказки | `title` с полным текстом | `visualAudit.test.ts` «music rows and upload lines…» |
| Правая панель участников | Имена обрезаются без подсказки | `title` с полным именем | `visualAudit.test.ts` «truncated names…» |
| Левая панель, люди в комнате; заголовок голосовой комнаты | то же | `title` | там же |
| Текстовый канал: тема канала, цитата ответа, текст ошибки отправки | то же | `title` | там же |
| Режим Finch, нижняя полоса: «В гнезде: …» | то же | `title` | `visualAudit.test.ts` «perch shows the full nest name…» |

Не найдено (проверено автоматически на всех экранах и четырёх темах): кнопок, полей и списков с браузерным стилем; текста цвета по умолчанию; вылезающего текста без многоточия в плеере, саундборде, всех страницах настроек, контекстных меню и в голосовой комнате на 12 участников.

Ограничения проверки: проверка цвета ограничена теми, что видны как цвет по умолчанию браузера (произвольный цвет вне переменных темы автоматически не найти); окно 1920×1080 снято только в теме claude; настоящий клиент Tauri (шрифты, масштаб Windows 125 %) не запускался, это остаётся в «Не проверено человеком».
