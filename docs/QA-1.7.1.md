# QA 1.7.1

## Root causes

- A1 (sound set choice does not apply in the settings modal): the z-index token `--z-popover` is declared only under `[data-theme="custom"]`, so in the default `claude` theme `.floating-panel` (the Select list, portalled to body) has `z-index: auto` and paints under `.overlay` (`z-index: 50`); clicks on the options land on the overlay, which closes the settings, instead of choosing the sound set. Fixed by declaring the depth scale at `:root` in `app.css`.
- A2 (Appearance page clipped on the left): only in «Мои цвета», the swatch's hidden colour input (`.theme-color .swatch input`, `inset:-8px`) and its focus ring overhang the swatch, which sits at the left edge of `.settings-page` (`overflow:auto`, `padding:0 4px 0 0` in `themes.css`), so they are clipped; Claude Dark, Снегирь and Finch Custom at 900×600 and 1280×800 show nothing clipped. Fixed by giving `.settings-page` 8px of left padding.
