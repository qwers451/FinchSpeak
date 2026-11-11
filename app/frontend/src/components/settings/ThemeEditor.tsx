import { createSignal, For, onCleanup, Show } from 'solid-js';
import { prefs, updateAppearance } from '../../stores/settingsStore';
import {
  contrast, DEFAULT_THEME_COLORS, isHexColor, paletteFromImage, THEME_COLOR_KEYS, THEME_PRESETS, type ThemeColors,
} from '../../stores/themeColors';
import { Button, Icon } from '../../ui';
import { t } from '../../i18n';

const LABELS: Record<keyof ThemeColors, [string, string]> = {
  background: ['Background', 'Chat and main area'],
  sidebar: ['Side panels', 'Channels and members'],
  surface: ['Cards and fields', 'Inputs, menus, the message box'],
  text: ['Text', 'Messages and labels'],
  accent: ['Accent', 'Buttons and selected items'],
  signal: ['Signal', 'Speaking, online, live'],
};

const sameColors = (a: ThemeColors, b: ThemeColors) => THEME_COLOR_KEYS.every((key) => a[key].toLowerCase() === b[key].toLowerCase());

function ColorField(props: { name: keyof ThemeColors }) {
  const colors = () => prefs().appearance.customColors;
  const [draft, setDraft] = createSignal<string | null>(null);
  const set = (value: string) => updateAppearance({ customColors: { ...colors(), [props.name]: value.toLowerCase() } });
  const [label, hint] = LABELS[props.name];
  return (
    <div class="theme-color">
      <label class="swatch" title={t(label)}>
        <i style={{ '--swatch-color': colors()[props.name] }} />
        <input type="color" aria-label={t(label)} value={colors()[props.name]} onInput={(event) => set(event.currentTarget.value)} />
      </label>
      <div class="col grow">
        <strong class="fs13">{t(label)}</strong>
        <span class="t3 fs12">{t(hint)}</span>
      </div>
      <input
        class="hex"
        aria-label={t('{name} as a hex code', { name: t(label) })}
        spellcheck={false}
        maxLength={7}
        value={draft() ?? colors()[props.name]}
        aria-invalid={draft() !== null && !isHexColor(draft())}
        onInput={(event) => {
          let value = event.currentTarget.value.trim();
          if (value && !value.startsWith('#')) value = `#${value}`;
          setDraft(value);
          if (isHexColor(value)) set(value);
        }}
        onBlur={() => setDraft(null)}
      />
    </div>
  );
}

/** A tile that turns a picture (chosen or dropped on it) into a palette. */
function FromImageTile() {
  let input: HTMLInputElement | undefined;
  const [preview, setPreview] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal('');
  const [over, setOver] = createSignal(false);
  onCleanup(() => { const url = preview(); if (url) URL.revokeObjectURL(url); });

  const use = async (file: File | undefined) => {
    if (!file) return;
    if (!file.type.startsWith('image/')) { setError(t('That is not a picture.')); return; }
    setBusy(true);
    setError('');
    try {
      const palette = await paletteFromImage(file);
      const old = preview();
      if (old) URL.revokeObjectURL(old);
      setPreview(URL.createObjectURL(file));
      updateAppearance({ customColors: palette });
    } catch {
      setError(t('Could not read this picture.'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
    <button
      type="button"
      class="theme-preset from-image"
      classList={{ over: over(), busy: busy() }}
      aria-label={t('Make a theme from a picture')}
      onClick={() => input?.click()}
      onDragOver={(event) => { event.preventDefault(); setOver(true); }}
      onDragLeave={() => setOver(false)}
      onDrop={(event) => { event.preventDefault(); setOver(false); void use(event.dataTransfer?.files[0]); }}
    >
      <span class="chips picture" aria-hidden="true">
        <Show when={preview()} fallback={<Icon name="image" size="sm" />}>{(url) => <img src={url()} alt="" />}</Show>
      </span>
      {error() || (busy() ? t('Reading colors…') : t('From a picture'))}
    </button>
    <input ref={input} type="file" accept="image/*" hidden onChange={(event) => { void use(event.currentTarget.files?.[0]); event.currentTarget.value = ''; }} />
    </>
  );
}

/** Pick six colors (or start from a ready palette); the whole app repaints as you go. */
export function ThemeEditor() {
  const colors = () => prefs().appearance.customColors;
  const textContrast = () => Math.min(contrast(colors().text, colors().background), contrast(colors().text, colors().surface));
  const accentContrast = () => contrast(colors().accent, colors().background);
  return (
    <div class="theme-editor">
      <div class="col g8">
        <span class="field-label">{t('Start from a palette')}</span>
        <div class="theme-presets">
          <For each={THEME_PRESETS}>
            {(preset) => (
              <button type="button" class="theme-preset" aria-pressed={sameColors(colors(), preset.colors)} onClick={() => updateAppearance({ customColors: preset.colors })}>
                <span class="chips" aria-hidden="true">
                  <For each={['sidebar', 'background', 'surface', 'accent', 'signal'] as const}>{(key) => <i style={{ 'background-color': preset.colors[key] }} />}</For>
                </span>
                {t(preset.name)}
              </button>
            )}
          </For>
          <FromImageTile />
        </div>
      </div>

      <div class="col g8">
        <div class="row between">
          <span class="field-label">{t('Colors')}</span>
          <Button size="sm" variant="ghost" icon="refresh" disabled={sameColors(colors(), DEFAULT_THEME_COLORS)} onClick={() => updateAppearance({ customColors: DEFAULT_THEME_COLORS })}>{t('Reset colors')}</Button>
        </div>
        <div class="theme-colors">
          <For each={THEME_COLOR_KEYS}>{(name) => <ColorField name={name} />}</For>
        </div>
      </div>

      <Show when={textContrast() < 4.5}>
        <div class="auth-banner amber" role="note"><Icon name="warn" /><span class="fs13">{t('The text is hard to read on these backgrounds. Make it lighter or the backgrounds darker (or the other way round).')}</span></div>
      </Show>
      <Show when={accentContrast() < 1.8}>
        <div class="auth-banner amber" role="note"><Icon name="warn" /><span class="fs13">{t('Buttons blend into the background. Pick a brighter or darker accent.')}</span></div>
      </Show>
    </div>
  );
}
