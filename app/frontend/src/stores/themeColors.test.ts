import { afterEach, expect, it, vi } from 'vitest';
import { applyAppearance, DEFAULT_PREFS, parsePrefs, THEME_BLEND_MS } from './settingsStore';
import {
  contrast, DEFAULT_THEME_COLORS, mix, paletteFromPixels, parseThemeColors, readableOn, THEME_PRESETS, THEME_VARIABLE_NAMES, themeVariables,
} from './themeColors';

afterEach(() => vi.useRealTimers());

it('mixes colors and measures contrast like WCAG', () => {
  expect(mix('#000000', '#ffffff', 0.5)).toBe('#808080');
  expect(mix('#102030', '#102030', 0.7)).toBe('#102030');
  expect(contrast('#000000', '#ffffff')).toBeCloseTo(21);
  expect(contrast('#777777', '#777777')).toBeCloseTo(1);
  expect(readableOn('#ffc400')).toBe('#16130f');
  expect(readableOn('#2e38ff')).toBe('#ffffff');
});

it('saved colors are repaired, not trusted', () => {
  expect(parseThemeColors(null)).toEqual(DEFAULT_THEME_COLORS);
  expect(parseThemeColors({ background: '#ABCDEF', accent: 'red', text: '#12345' })).toEqual({ ...DEFAULT_THEME_COLORS, background: '#abcdef' });
  const prefs = parsePrefs(JSON.stringify({ appearance: { theme: 'user', customColors: { signal: '#00ff00' } } }));
  expect(prefs.appearance.theme).toBe('user');
  expect(prefs.appearance.customColors.signal).toBe('#00ff00');
  expect(parsePrefs(JSON.stringify({ appearance: { theme: 'neon' } })).appearance.theme).toBe('claude');
});

it('every palette keeps its text readable and its buttons labelled clearly', () => {
  for (const { name, colors } of THEME_PRESETS) {
    const vars = themeVariables(colors);
    expect(contrast(colors.text, colors.background), name).toBeGreaterThanOrEqual(7);
    expect(contrast(vars['--text-3'], colors.background), name).toBeGreaterThanOrEqual(3);
    expect(contrast(vars['--on-accent'], colors.accent), name).toBeGreaterThanOrEqual(3);
  }
});

it('my colors paint the page, and another theme takes them away again', () => {
  vi.useFakeTimers();
  const root = document.createElement('div');
  const ocean = THEME_PRESETS.find((preset) => preset.id === 'ocean')!.colors;
  applyAppearance({ ...DEFAULT_PREFS.appearance }, root);
  expect(root.dataset.themeBlend).toBeUndefined();

  applyAppearance({ ...DEFAULT_PREFS.appearance, theme: 'user', customColors: ocean }, root);
  expect(root.dataset.theme).toBe('user');
  expect(root.style.getPropertyValue('--bg-app')).toBe(ocean.background);
  expect(root.style.getPropertyValue('--yellow')).toBe(ocean.accent);
  expect(root.dataset.themeBlend).toBe('true');
  vi.advanceTimersByTime(THEME_BLEND_MS);
  expect(root.dataset.themeBlend).toBeUndefined();

  applyAppearance({ ...DEFAULT_PREFS.appearance, theme: 'claude', customColors: ocean }, root);
  for (const name of THEME_VARIABLE_NAMES) expect(root.style.getPropertyValue(name)).toBe('');
});

it('colors switch at once when motion is reduced', () => {
  const root = document.createElement('div');
  applyAppearance({ ...DEFAULT_PREFS.appearance }, root);
  applyAppearance({ ...DEFAULT_PREFS.appearance, theme: 'user', reduceMotion: true }, root);
  expect(root.dataset.themeBlend).toBeUndefined();
});

const pixels = (...blocks: [number, number, number, number][]) =>
  blocks.flatMap(([r, g, b, count]) => Array.from({ length: count }, () => [r, g, b, 255]).flat());
const hueOf = (hex: string) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  const max = Math.max(r, g, b);
  return max === r ? 'red' : max === g ? 'green' : 'blue';
};

it('a picture gives a dark theme in its main hue with its vivid color as the accent', () => {
  const palette = paletteFromPixels(pixels([20, 40, 120, 700], [240, 120, 20, 200], [30, 200, 90, 100]));
  expect(hueOf(palette.background)).toBe('blue');
  expect(hueOf(palette.accent)).toBe('red');
  expect(hueOf(palette.signal)).toBe('green');
  expect(contrast(palette.text, palette.background)).toBeGreaterThanOrEqual(7);
  expect(contrast(palette.accent, palette.background)).toBeGreaterThanOrEqual(3);
});

it('a light picture gives a light theme, and an empty one the defaults', () => {
  const palette = paletteFromPixels(pixels([245, 240, 230, 900], [200, 60, 40, 100]));
  expect(contrast(palette.background, '#ffffff')).toBeLessThan(1.2);
  expect(contrast(palette.text, palette.background)).toBeGreaterThanOrEqual(7);
  expect(paletteFromPixels([0, 0, 0, 0])).toEqual(DEFAULT_THEME_COLORS);
});

it('a grey picture still gets a colorful accent and a different signal', () => {
  const palette = paletteFromPixels(pixels([90, 90, 90, 1000]));
  expect(palette.accent).not.toBe(palette.signal);
  expect(contrast(palette.text, palette.background)).toBeGreaterThanOrEqual(7);
});
