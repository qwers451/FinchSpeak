# QA 1.7.1

## Root causes

- A1 (sound set choice does not apply in the settings modal): the z-index token `--z-popover` is declared only under `[data-theme="custom"]`, so in the default `claude` theme `.floating-panel` (the Select list, portalled to body) has `z-index: auto` and paints under `.overlay` (`z-index: 50`); clicks on the options land on the overlay, which closes the settings, instead of choosing the sound set. Fixed by declaring the depth scale at `:root` in `app.css`.
- A2 (Appearance page clipped on the left): only in «Мои цвета», the swatch's hidden colour input (`.theme-color .swatch input`, `inset:-8px`) and its focus ring overhang the swatch, which sits at the left edge of `.settings-page` (`overflow:auto`, `padding:0 4px 0 0` in `themes.css`), so they are clipped; Claude Dark, Снегирь and Finch Custom at 900×600 and 1280×800 show nothing clipped. Fixed by giving `.settings-page` 8px of left padding.
- A3 (music panel looked unstyled): MusicPanel.tsx uses `tabular` and `music-queue-block`, which have no rule in music.css or global CSS (`tabular` is defined nowhere), and the now-playing block, tabs and empty state had no shared-style treatment; music.css is imported in index.tsx and the other panel classes match. Fixed by card styling for `.music-now`, `.music-clock`, `.music-queue-block`, shared `.tabs`/`.tab` and a FinchBird empty state.
- A4 (sent messages look "crooked"): the echo replaces `local-<client_msg_id>` with the server id, so `ChatArea` (keyed by `message.id`) unmounted the pending row and mounted a new one that replayed `msg-enter`; additionally `msg-enter` was set via `classList` beside a dynamic `class` string, so the `sending` to stored change stripped it mid-animation. Fixed by keying rows by `client_msg_id` and putting `msg-enter` in the class string. Scroll-to-bottom after own send was already correct (transforms do not change scrollHeight).

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

Counts: 16 good, 3 fixed (rows 5, 6, 7), 0 removed.

Open concern (not changed): when the room has more people than tiles, the speaking set decides which tiles show, so a change of who is speaking reorders the grid and the tiles glide each time. This is by design of `arrange`; no frame capture was taken to judge how often it happens.
