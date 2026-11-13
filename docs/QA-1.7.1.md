# QA 1.7.1

## Root causes

- A1 (sound set choice does not apply in the settings modal): the z-index token `--z-popover` is declared only under `[data-theme="custom"]`, so in the default `claude` theme `.floating-panel` (the Select list, portalled to body) has `z-index: auto` and paints under `.overlay` (`z-index: 50`); clicks on the options land on the overlay, which closes the settings, instead of choosing the sound set. Fixed by declaring the depth scale at `:root` in `app.css`.
